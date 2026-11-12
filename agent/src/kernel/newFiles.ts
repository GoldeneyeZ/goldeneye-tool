import { execFile } from "node:child_process";

const MAX_FILES = 400;

/**
 * Files added to the working tree and not committed yet: untracked ones and
 * ones added relative to HEAD. A file created after the last index run is not
 * in the graph, so its symbols can only be found by reading it.
 */
export class NewFiles {
  private pending: Promise<string[]> | undefined;
  private everything: Promise<string[]> | undefined;

  constructor(private readonly root: string) {}

  /**
   * Every file of the working tree that git does not ignore. The index skips
   * some of them (build scripts, languages without a grammar, large data).
   */
  all(): Promise<string[]> {
    this.everything ??= this.git(["ls-files", "--cached", "--others", "--exclude-standard"]);
    return this.everything;
  }

  list(): Promise<string[]> {
    this.pending ??= Promise.all([
      this.git(["ls-files", "--others", "--exclude-standard"]),
      this.git(["diff", "--name-only", "--diff-filter=A", "HEAD"]),
    ]).then(([untracked, added]) => [...new Set([...untracked, ...added])].slice(0, MAX_FILES));
    return this.pending;
  }

  /** Files that differ from HEAD and still exist: modified, added, renamed and untracked ones. */
  async changed(): Promise<string[]> {
    const [modified, untracked] = await Promise.all([
      this.git(["diff", "--name-only", "--diff-filter=d", "HEAD"]),
      this.git(["ls-files", "--others", "--exclude-standard"]),
    ]);
    return [...new Set([...modified, ...untracked])];
  }

  /** The committed text of a file, or undefined when HEAD does not have it. */
  atHead(filePath: string): Promise<string | undefined> {
    return new Promise((resolve) => {
      execFile(
        "git",
        ["-C", this.root, "show", `HEAD:${filePath}`],
        { maxBuffer: 16 * 1024 * 1024, windowsHide: true },
        (error, stdout) => resolve(error ? undefined : stdout),
      );
    });
  }

  private git(args: string[]): Promise<string[]> {
    return new Promise((resolve) => {
      execFile(
        "git",
        // Paths as they are on disk, not octal-escaped: other commands read them back.
        ["-C", this.root, "-c", "core.quotepath=off", ...args],
        { maxBuffer: 64 * 1024 * 1024, windowsHide: true },
        // Not a git checkout, or no git: there is simply nothing to add.
        (error, stdout) => resolve(error ? [] : stdout.split(/\r?\n/).filter(Boolean)),
      );
    });
  }
}

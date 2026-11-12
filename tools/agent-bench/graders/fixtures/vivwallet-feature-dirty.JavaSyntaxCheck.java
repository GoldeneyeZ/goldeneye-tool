import com.sun.source.util.JavacTask;

import javax.tools.Diagnostic;
import javax.tools.DiagnosticCollector;
import javax.tools.JavaCompiler;
import javax.tools.JavaFileObject;
import javax.tools.StandardJavaFileManager;
import javax.tools.ToolProvider;
import java.io.File;
import java.nio.charset.StandardCharsets;
import java.nio.file.Files;
import java.nio.file.Path;
import java.nio.file.Paths;
import java.util.List;
import java.util.Locale;
import java.util.stream.Collectors;
import java.util.stream.Stream;

/**
 * Parse-only Java syntax check used by graders/vivwallet-feature-dirty.mjs.
 *
 * usage: java vivwallet-feature-dirty.JavaSyntaxCheck.java &lt;release&gt; &lt;source root&gt;
 *
 * Parses every .java file under the source root with the JDK compiler at the
 * given language level. No classpath is needed because nothing is resolved:
 * only syntax errors (and language features newer than the release) are
 * reported. Exit code 0 = all files parse, 1 = syntax errors, 2 = usage.
 */
public class JavaSyntaxCheck {

    public static void main(String[] args) throws Exception {
        if (args.length != 2) {
            System.err.println("usage: JavaSyntaxCheck <release> <source root>");
            System.exit(2);
        }
        Path root = Paths.get(args[1]);
        List<File> files;
        try (Stream<Path> paths = Files.walk(root)) {
            files = paths.filter(path -> path.toString().endsWith(".java"))
                         .map(Path::toFile)
                         .collect(Collectors.toList());
        }
        if (files.isEmpty()) {
            System.err.println("no .java file under " + root);
            System.exit(2);
        }
        JavaCompiler compiler = ToolProvider.getSystemJavaCompiler();
        DiagnosticCollector<JavaFileObject> diagnostics = new DiagnosticCollector<>();
        int errors = 0;
        try (StandardJavaFileManager fileManager =
                     compiler.getStandardFileManager(diagnostics, Locale.ROOT, StandardCharsets.UTF_8)) {
            JavacTask task = (JavacTask) compiler.getTask(null,
                                                          fileManager,
                                                          diagnostics,
                                                          List.of("-proc:none", "--release", args[0]),
                                                          null,
                                                          fileManager.getJavaFileObjectsFromFiles(files));
            task.parse();
            for (Diagnostic<? extends JavaFileObject> diagnostic : diagnostics.getDiagnostics()) {
                if (diagnostic.getKind() != Diagnostic.Kind.ERROR) {
                    continue;
                }
                errors++;
                String name = diagnostic.getSource() == null
                              ? "<unknown>"
                              : root.relativize(Paths.get(diagnostic.getSource().toUri())).toString();
                System.out.println(name + ":" + diagnostic.getLineNumber() + ": " + diagnostic.getMessage(Locale.ROOT));
            }
        }
        System.out.println(files.size() + " files parsed, " + errors + " syntax error(s)");
        System.exit(errors == 0 ? 0 : 1);
    }

}

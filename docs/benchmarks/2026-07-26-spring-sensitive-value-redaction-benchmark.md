# Spring sensitive-value redaction benchmark

Date: 2026-07-26

## Outcome

The benchmark harness, clean-agent execution controls, immutable snapshots,
held-out graders, qualification gates, and Level 2 → Level 1 → Level 0 workload
ladder were implemented and verified.

The final accepted workload produced 993,518 vanilla input tokens and passed
the held-out grader. It missed the original 100,000 uncached-input gate with
62,702 uncached tokens. The user explicitly accepted that shortfall and ended
further tuning after the final clean paired run.

No randomized 3×3 matrix was executed. Results below are one clean ephemeral
run per lane (`n = 1`), so sample SD, CV, confidence intervals, and statistical
significance are unavailable.

## Frozen final provenance

- Candidate: `ba5876e693e580947481a166cba8910f7e81a9df`
- Spring source: `daf955157871e4ac6f192e06b71d6cc595eb979b`
- Task hash: `f417d250dc4528962182d7b5c53f8fe0dff81949ea67f392f1e0459d436509db`
- Grader hash: `623510614e4f704ebec715eaa0a96caba28c48ace23205afa535168a3e07a1b8`
- Snapshot manifest:
  `46824ab410a1fe95df216c6092f491164eeb2867f26bde54e8e1cb39edd3095c`
- Goldeneye binary SHA-256:
  `62fa214bf146ef99dfb3ae9236299934ee0c25190fc921ad1928ff8581d885de`

Both final lanes used clean ephemeral Codex agents, the same task, source,
snapshot, and held-out grader. Both passed with zero protocol violations and
passed the dirty-path policy.

## Final paired result

| Metric | Vanilla | Goldeneye/ACK | ACK delta |
|---|---:|---:|---:|
| Grader | PASS | PASS | — |
| Input tokens | 993,518 | 984,009 | -9,509 (-0.96%) |
| Cached input | 930,816 | 935,424 | +4,608 |
| Uncached input | 62,702 | 48,585 | -14,117 (-22.51%) |
| Output tokens | 9,601 | 9,234 | -367 |
| Total tokens | 1,003,119 | 993,243 | -9,876 (-0.98%) |
| Tool calls | 15 | 33 | +18 |
| Patch files | 5 | 5 | 0 |
| Wall time | 5.29 min | 7.16 min | +35.50% |
| Protocol violations | 0 | 0 | 0 |

ACK reduced final total tokens by 0.98%, but required 2.2× the tool calls and
35.50% more wall time. At `n = 1`, this is directional evidence only.

## Workload ladder

Each calibration below used a new clean vanilla agent and passed its held-out
grader.

| Workload | Candidate | Input | Cached | Uncached | Wall time | Gate result |
|---|---|---:|---:|---:|---:|---|
| Level 2 | `40d86b0` | 4,840,885 | 4,715,264 | 125,621 | 16.11 min | Above 1.2M |
| Level 1 | `4c61830` | 4,475,886 | 4,312,320 | 163,566 | 18.31 min | Above 1.2M |
| Level 0 basic | `f71dd98` | 1,083,262 | 1,018,880 | 64,382 | 6.87 min | Uncached below 100k |
| Level 0 nested/indexed | `e01570a` | 1,158,007 | 1,070,592 | 87,415 | 4.92 min | Uncached below 100k |
| Level 0 composed/final | `ba5876e` | 993,518 | 930,816 | 62,702 | 5.29 min | Accepted exception |

Level 0 nested/indexed produced the best uncached result inside the total-input
band: 87,415 uncached at 1,158,007 total input.

## Cross-level ACK comparison

| Workload | Vanilla input | ACK input | ACK input delta | Vanilla wall | ACK wall |
|---|---:|---:|---:|---:|---:|
| Level 2 | 4,840,885 | 5,367,988 | +10.89% | 16.11 min | 28.57 min |
| Level 1 | 4,475,886 | 3,850,540 | -13.97% | 18.31 min | 20.34 min |
| Level 0 final | 993,518 | 984,009 | -0.96% | 5.29 min | 7.16 min |

ACK was slower in every observed pair. Its input-token effect changed sign
across workloads, from a 10.89% regression to a 13.97% reduction.

## Interpretation

Provider caching dominated input:

- Level 2 vanilla: 97.40% cached.
- Level 2 ACK: 97.66% cached.
- Final Level 0 vanilla: 93.69% cached.
- Final Level 0 ACK: 95.06% cached.

Level 2 ACK and vanilla consumed nearly identical uncached input (125,876
versus 125,621). ACK's additional 527,103 input tokens came almost entirely
from repeated cached prefixes. It made 79 tool calls versus vanilla's 36.

The dominant cost driver was round-trip count and repeated growing context,
not task scope alone. Reducing vanilla tool calls from 36 at Level 2 to 15 at
final Level 0 reduced cumulative input by 79.48%, while uncached input fell by
50.09%.

## Limitations

- One run per lane; no variance estimate.
- ACK lane was the mandatory unscored smoke, paired with vanilla calibration.
- Provider cache behavior affects raw-token interpretation.
- Raw `target/agent-bench/**` artifacts remain local and are intentionally not
  committed.
- No claim of statistical significance.

## Verification

- Harness tests: 46 passed, 0 failed.
- Final ACK grader: PASS.
- Final vanilla grader: PASS.
- Candidate repository: clean after benchmark.
- Spring repository: clean at pinned commit.
- Final snapshot restore verification: PASS.


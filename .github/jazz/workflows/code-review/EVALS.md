# First-pass review regression cases

`review-cases.json` records the concrete misses and unsupported findings observed on PR #74. It is a small regression set, not evidence that a reviewer can find every possible bug.

For a matched replay, use a fresh checkout at each case's head, its frozen base/head diff and the PR description before its first review. Exclude later comments, review outputs, this case manifest and expected findings from the reviewer's context. Inject the workflow being evaluated with the same `ci-reviewer` model, reasoning, persona, tools, read-only policy and 50-iteration limit as CI. Preserve the strict two-block output format. Run locally without posting to a pull request.

Score the complete first response against these dimensions:

- Recall: which known defects were identified before any follow-up or fix?
- Precision: which claims are supported by an actual failure path, and which rely on incorrect API/platform assumptions?
- Coverage: did the review check relevant unchanged callers and resource stages, or explicitly report an incomplete/limited review?
- Format and policy: are comment anchors valid, are the two output blocks parseable, and were prohibited commands avoided?

Validate any new finding independently rather than treating the expected list as exhaustive. For platform or standard-library claims, use the applicable implementation and native evidence listed in the manifest; never replace the read-only policy with permission to execute PR code. Record the model, limits, output, tool/coverage failures, cost when reported and validation limits with each result. Repeat matched runs before claiming a reliable improvement: a single successful replay checks one sample, not recall in general.

## Recorded local replays

On 2026-11-12, an isolated copy of the deployed CI configuration (ChatGPT GPT-6 Luna, medium reasoning, coder persona, identical tools, read-only policy and 50-iteration limit) reviewed the frozen first case with the initial revised instructions. It found two of the five known defect families, missed the unchanged screenshot caller and startup/active termination gaps, and returned malformed outer fences. It did not emit the three unsupported platform/API findings, but misstated the CLI package name in one valid finding. The reported cost was $0.23587652. This run did not meet the target.

That result motivated the deterministic caller/package/resource inventory and clearer output and evidence requirements. A second matched replay with the inventory stopped at the provider usage limit without a verdict; its reported cost was $0.3465625. It provides no recall or precision result. The final collector also has offline Git-snapshot regressions for merge-base selection, compact selectors, Unicode filenames and dirty-working-tree isolation. The final workflow has not yet completed a matched model replay, so no measured improvement or reliable-recall claim is made.

The CI preflight executes the collector extracted from the PR base revision with isolated Python. It reads the proposed head through Git, rather than running the proposed collector. While the introducing PR's base has no collector yet, CI provides an explicit unavailable-inventory context and the reviewer uses manual searches; after merge, later PRs use the base's deployed collector.

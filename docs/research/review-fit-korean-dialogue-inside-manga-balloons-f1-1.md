# Review research — F1, cycle 1

Draft research before the fix of review round 4 of PR #9, written 2026-11-21. It is not a verified fact or a shared rule: the review reads it with the fix.

## Problem

F1 (rounds 1-4, P1, docs/design/backend.md:144-154): the review cannot check, from the reviewed head itself, the runtime verification the contract requires for the balloon-fit change. Required are head-bound isolated API assertions, actual browser actions, native-host (Windows/Android) observations, focused offline results, and the Galaxy S21 three-chapter read-through with matched crops, the per-attempt Gemini ledger and approvals. Invariant: every verification claim in a review-round report must rest on evidence the reviewer can inspect in the reviewed tree and can tie to that exact commit's runtime inputs. Prose in a disposition does not count.

Trigger: recurrence, rounds 1, 2, 3, 4.

- [P1] [P1] docs/design/backend.md:154 — Required runtime verification is still missing for the reviewed head.

## Failed fixes

- Round 1 (30af69c, marked fixed): the report cited a Galaxy S21 walk on build d6c1370 (61 page visits, 262 detected leaks) plus a phone script, crops, failure log, attempt ledger and approvals. All of these live in git-ignored artifacts/ (balloonfit-phone.mjs, balloonfit-*.log, balloonfit.apk), so the review tools could not open them. Nothing tied d6c1370's inputs to 30af69c. The only server receipt was git diff --check.
- Round 2 (29dbf21, marked disagree): an offline replay and focused checks on 2ad8e91, with an equivalence claim back to the older phone build. The claim was prose, it was about an earlier commit than the head under review, and it admitted the S21 walk was still pending. Disagreeing changed no evidence, so the same finding came back.
- Round 3 (a5821bf, marked fixed): a 61-page replay that compares the backend's layout JSON. The same round changed the renderer (tone patches decoded through createImageBitmap, overflowing Korean scaled by a CSS transform; backend.md:150,154). Identical JSON therefore no longer shows the reader draws the same thing. The phone walk and signoff still named an earlier build, and the ledger was summarised as totals with no per-attempt records.
- Round 4 (00868b4): an Edge walk over 61 cached pages, isolated Gemini-stub tests and focused offline checks were reported. The walk script, crops and ledger again stayed uncommitted. The physical S21 read-through still named an earlier APK, and no current-head native-host interaction was recorded.

## Sources

### S1 — SLSA v1.2 — Build: Verifying artifacts

- URL: https://slsa.dev/spec/v1.2/verifying-artifacts
- Retrieved: 2026-11-21
- Locator: Step 1 item 2; Step 3: (Optional) Check dependencies recursively
- Fragment: "Verify that statement's subject matches the digest of the artifact in question." ... "Step 3: (Optional) Check dependencies recursively" ... "Note that SLSA v1.0 does not have any requirements on the completeness or verification of resolvedDependencies." "If resolvedDependencies is incomplete, these checks can be done on a best-effort basis."
- Applicability: The reviewer checks evidence against the reviewed head by digest. The commit binding is optional in SLSA, so the receipt must add it explicitly (git diff --exit-code to HEAD).
- Counterevidence: SLSA is about build provenance, not manual device walks. It applies by analogy to test evidence.
- Validation: check-evidence.mjs fails when a cited file's hash or a runtime input differs from the reviewed head.

### S2 — in-toto Attestation — Test Result predicate

- URL: https://github.com/in-toto/attestation/blob/main/spec/predicates/test-result.md
- Retrieved: 2026-11-21
- Locator: subject; configuration; url; Use cases
- Fragment: "The expected subject are the source artifacts tested." configuration: "Reference to the configuration used for the test run." Verification checks the attestation "used the right configurations" and "that all required tests passed." url: may help "to confirm the test was performed against the expected subject", but the predicate "makes no assumptions about the test harness or system" it points to. Names of passed and failed tests: "The semantics of the name must be determined separately between the producer and consumer." The spec has no explicit rule binding results to a source revision; its example uses a gitCommit digest as subject.
- Applicability: Each walk, test run and device read-through is a test result whose subject must be the reviewed head and whose configuration (APK hash, ledger mode, page set) must be recorded.
- Counterevidence: It is a schema, not a requirement on this repository's review. Binding to a revision is shown only by example.
- Validation: verification.txt names, for each run, the commit, the build hash and the configuration. check-evidence.mjs ties the commit to HEAD.

### S3 — Windows installer receipt and check (repository)

- URL: e2e/receipts/windows-installer/check-evidence.mjs
- Retrieved: 2026-11-21
- Locator: tests 'every cited desktop capture is readable from HEAD' and 'the installer build inputs match the reviewed tree'
- Fragment: Hashes each listed .png via `git show HEAD:<folder>/<name>`, asserts the receipt lists exactly those files, and runs `git diff --exit-code 4c3390c... HEAD -- . :(exclude,glob)<folder>/**`. verification.txt: 'This command runs from the repository root... compares the files outside this receipt directory with the build source.' The .exe itself is kept outside the tree, with its SHA-256 in the receipt.
- Applicability: A pattern this repo has already merged for exactly this class of evidence. The balloon-fit receipt can copy it.
- Counterevidence: Was it accepted because of this check, or for other reasons? No review record was read.
- Validation: The same structure, run on this branch's head, passes.

### S4 — Reader layouts receipt (repository)

- URL: e2e/receipts/reader-layouts/verification.txt
- Retrieved: 2026-11-21
- Locator: 'Current candidate binding'
- Fragment: 'Use the exact reviewed commit in place of HEAD when verifying a detached candidate... Its exact exclusions are documentation, generated wiki metadata, this evidence, and the Rust integration test, which is compiled and run separately.' `git diff --exit-code --no-renames f6061d9 HEAD -- . ':(exclude,literal).wiki/corpus.json' ...` 'Exit 0 permits reuse of the native packages after checking their hashes.'
- Applicability: Shows how to reuse an earlier native build legitimately: only when the diff of runtime inputs is empty. On this branch that fails for every build before a5821bf, because a5821bf changed the renderer.
- Counterevidence: none
- Validation: Running that diff from d6c1370 or 2ad8e91 to the head exits non-zero, which shows why the older walks could not count.

### S5 — .gitignore and artifacts listing (repository)

- URL: .gitignore
- Retrieved: 2026-11-21
- Locator: line 2; artifacts/balloonfit-*
- Fragment: .gitignore line 2: 'artifacts/'. Balloon-fit evidence on disk: artifacts/balloonfit-phone.mjs, balloonfit-small.mjs, balloonfit-web.log, balloonfit-rust.log, balloonfit-build.log, balloonfit-build.ps1, balloonfit-isolation.gradle, balloonfit.apk. There is no e2e/receipts/balloon-fit folder; receipts exist only for chapter-import, saved-lists, reader-layouts, windows-installer and korean-readme.
- Applicability: The scripts and logs the reports cited can never reach the reviewer's tree, which matches 'unavailable through the read-only tools' in rounds 1 and 4.
- Counterevidence: The ledger directory and crops may live outside artifacts/. Their location was not found.
- Validation: `git ls-files e2e/receipts/balloon-fit` lists the committed evidence after the fix.

### S6 — Backend design — In-app reader drawing (repository)

- URL: docs/design/backend.md
- Retrieved: 2026-11-21
- Locator: lines 146-154
- Fragment: 'Still over at 2 px, the lines are scaled down whole into the text area (a CSS transform)' and 'a .rst-patch (the tone fill, a canvas painted from the PNG's bytes through createImageBitmap, so a site whose image policy refuses data: addresses still shows it)'.
- Applicability: These parts of the renderer changed in round 3. Backend layout JSON does not capture them, so only browser and device runs on the current head show what the reader draws.
- Counterevidence: If the renderer change touched only pages the walk does not cover, older walks would still be valid for the rest. The review did not accept that.
- Validation: The Edge and S21 walks on the final head cover pages with patches and with overflowing Korean.

### S7 — Gemini request ledger (repository)

- URL: scripts/gemini-ledger.mjs
- Retrieved: 2026-11-21
- Locator: header comment, lines 6-15
- Fragment: 'any further attempt for it ... is refused until <ledger>/approvals.json grants it ... Every attempt, sent or not, is appended to <ledger>/attempts.jsonl, and GET /ledger sums them up. With LEDGER_OFFLINE=1 nothing is ever sent.'
- Applicability: The per-attempt records the round-3 review asked for already exist as files. Reports gave only the GET /ledger totals. Committing attempts.jsonl and approvals.json makes each repeat attempt and its approval inspectable.
- Counterevidence: The file may hold page hashes the person considers private. None of these contain the API key, since the key is passed in a header.
- Validation: check-evidence.mjs confirms that every 'outbound' entry with n>1 has a matching approvals.json grant.

## Claims

- C1 (source; S1, S2): Evidence counts for a candidate only when it names that candidate as its subject, by digest or commit, and records the configuration it ran under. A verifier compares both against what it expects.
- C2 (interpretation; S5, S3): All earlier fixes kept the balloon-fit evidence in git-ignored artifacts/, so the reviewer, who reads only the tree, could not inspect any of it. That is why each round reports the scripts, crops and ledger as unavailable.
- C3 (source; S3, S4): This repository already has an accepted pattern for this kind of evidence: commit e2e/receipts/<feature>/ with verification.txt, the captures, and a runnable check that hashes them from HEAD and runs git diff --exit-code from the run source to HEAD.
- C4 (interpretation; S1, S4, S6): Walks on builds before a5821bf cannot be reused for the current head, because the renderer change makes the runtime-input diff non-empty. Backend layout-JSON equality does not cover what the reader draws.
- C5 (interpretation; S7): The per-attempt ledger the review asked for is already produced as attempts.jsonl and approvals.json. Committing those files, not the totals, answers the round-3 point about individual repeat attempts.

## Core theory

Supply-chain verification ties evidence to an artifact by digest, and to a source revision by a recorded commit that the verifier compares against what it expects. SLSA: 'Verify that statement's subject matches the digest of the artifact in question.' SLSA also says source and dependency checks are optional ('SLSA v1.0 does not have any requirements on the completeness or verification of resolvedDependencies'), so binding to a commit has to be added deliberately. The in-toto test-result predicate makes a test run's subject 'the source artifacts tested' (its example uses a gitCommit digest). It records 'the configuration used for the test run' so verification can check it 'used the right configurations', and a policy checks that 'all required tests passed'.

## Applicability

The reviewer acts as the verifier, and the reviewed head is the subject. A report naming d6c1370 or 2ad8e91 is evidence about another subject unless a committed check shows those commits' runtime inputs equal the head's. Round 3 changed the renderer (src-tauri/src/reader.js drawing; backend.md:146-154), so that equality fails for any walk from before round 3. The fix belongs in e2e/receipts/balloon-fit/. Put verification.txt there with the exact commands. Add the walk script and the per-page crops for the 61 pages, with the leak-detector failure log. Add the ledger's attempts.jsonl and approvals.json (scripts/gemini-ledger.mjs writes both), so each repeat attempt and its approval can be inspected. Add the vitest and cargo JUnit or logs for the focused tests. Add check-evidence.mjs, modelled on e2e/receipts/windows-installer/check-evidence.mjs. It should hash every cited file from HEAD and run git diff --exit-code <run-source-commit> HEAD -- . with only docs, wiki metadata and the receipt folder excluded, as in reader-layouts/verification.txt. Every walk, browser, native-host and S21 run must be redone on the final commit, or on a commit that check proves equivalent. That means the fix commit has to be the last code change before the runs.

## Hypothesis

The cause is where the evidence is kept and how it is tied to a commit. It is not the amount of testing. Every round's evidence stayed in artifacts/, which .gitignore line 2 excludes, so the reviewer, who reads only the committed tree, saw claims and no artifacts. No round committed a check that binds the evidence to the reviewed head: a hash of each cited file plus a git diff --exit-code from the build or run source to HEAD over runtime inputs. The head moved every round (four commits, one of them a renderer change), so evidence from an earlier build went stale each time. Features this repo has already merged closed the same class of finding by committing e2e/receipts/<feature>/: verification.txt, the captures, and a runnable check-evidence.mjs that hashes each capture from HEAD and runs git diff --exit-code against the source commit. This branch has no e2e/receipts folder.

## Counterevidence

- The round-4 wording also names things that committing files alone does not cover: 'Current-head native-host interaction and the final installed-device read-through remain unverified.' The S21 read-through needs the person and the physical phone, so no agent turn can produce it alone.
- The review may need only the S21 read-through, and accept the rest as reported. Then committing receipts is necessary but not sufficient.
- Committed captures prove what was captured, not that the run happened as described. A reviewer may still treat them as self-reported. The merged features were accepted on the same kind of evidence, which argues for this path but does not guarantee it.
- Round 1 asked for 'head-bound isolated API assertions'. Some of that may be expected as a committed Rust integration test in src-tauri/tests rather than a receipt.

## Alternatives

- Run the walk again and report it in the disposition: tried in rounds 1, 3 and 4. Without committed artifacts the reviewer sees the same thing, a claim.
- Disagree on scope (round 2): did not move the review, because the contract lists the runtime evidence as required.
- Upload the evidence to a GitHub release or CI artifact: reachable by URL, but the review tools read the tree. The windows-installer precedent kept the .exe outside the tree and committed only its hashes and the captures. Use that split for the APK: hash in the tree, binary handed over separately.
- Turn the walk into a gate test: the gate cannot drive the Android WebView or a physical S21 (.wiki/mobile-first), so it cannot replace the device evidence.
- Freeze the head before verifying (no code change after the runs): necessary as part of the fix rather than a separate option. Every round so far changed code after the evidence was taken.

## Experiment

On the final candidate commit C (no later code change): (1) build the APK and Windows installer from C and record their SHA-256. (2) Run the Edge walk over the 61 cached pages, the isolated web_host_files API tests, the focused vitest and cargo tests, and the Windows installed-app journey, with the Gemini ledger offline or approval-gated. Save the outputs, crops, attempts.jsonl and approvals.json into e2e/receipts/balloon-fit/. (3) Have the person install the C-built APK on the S21 and read the three chapters. Commit their dated signoff, the device captures, and the installed APK hash as aapt or adb reports it. (4) Commit check-evidence.mjs. Then run `node --test e2e/receipts/balloon-fit/check-evidence.mjs` at the reviewed head: it must pass, and it must fail if one cited file is altered or one runtime input is changed. The hypothesis is confirmed if round 5's review can open each cited file and stops raising F1. It is refuted if the review still raises F1 against receipts that are committed and pass that check. In that case the missing piece is something else, most likely the person's device signoff or a committed integration test.

## Result

pending — the fix records what it observed here, with the tests and the commit.

## Unresolved limits

Whether the reviewer accepts committed captures and a person's signoff as satisfying 'native-host observations' and 'installed-device read-through' is not known. The merged precedents suggest it does. The S21 read-through needs the person and cannot be scheduled from this turn. Large binaries (the APK, 61 pages of crops) may push repository size limits. Hashes plus a subset of crops may be needed, and whether the reviewer accepts a subset is uncertain. This research ran nothing. The claim that balloonfit evidence exists only under artifacts/ rests on a file listing, not on reading the earlier round reports.

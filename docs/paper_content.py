"""
Orion research paper — content source.

Every number in this file was measured on this codebase, not estimated.
The claim each number supports is recorded beside it so that a reader can
re-derive it from the repository.
"""

TITLE = "Orion: Single-Resident Local AI with a Two-Domain Capability Boundary for 8 GB Workstations"

AUTHORS = ["Kuldeep Yadav"]

AFFILIATION = "Independent Research and Development"

ABSTRACT = [
    "Cloud language-model APIs are excellent at capability and poor at custody. The "
    "alternative — a model that runs on the machine it serves — is usually presented "
    "as a smaller, weaker version of the cloud service. It is a different "
    "engineering problem, one whose hard constraints are physical rather than "
    "algorithmic, and its solutions should not be judged by the cloud service's "
    "metrics.",
    "We describe Orion, a desktop AI workspace built across eleven milestones "
    "(M0–M10). It runs a quantized open-weight model entirely on an 8 GB "
    "workstation, ingests documents without a network round trip, and drives the "
    "host operating system and a web browser on the user's direct instruction. "
    "Exactly one model occupies RAM at a time, and untrusted retrieved content is "
    "separated from privileged instructions by construction rather than by prompt "
    "wording.",
    "We report five contributions reusable independently of the product: a "
    "path-difference residency rule that decides model swaps by comparing resolved "
    "weight files rather than role labels; a two-domain capability broker that "
    "refuses privileged calls whose provenance traces to retrieved content; "
    "byte-budget resource accounting, in which every ingestion cap is enforced on "
    "bytes actually read; a compile-time assertion that makes a panic-catching "
    "guard impossible to ship under an aborting build profile, a defect our own "
    "test suite masked for a full milestone; and heading-trail chunking, worth "
    "+0.154 recall@4 overall and +0.666 on heading-scoped probes.",
    "On a 30-question adversarial retrieval suite we measure 0.769 recall@4 and 0.667 "
    "MRR for hybrid retrieval against 0.731 and 0.635 for BM25 alone. Our ingestion "
    "pipeline refuses a 600 MB zip bomb declared inside 600 KB in 311 ms with a 3 MB "
    "resident-set increase. We report the defect history alongside the results, "
    "because the defects, not the features, are what a reader needs in order to "
    "judge the claims.",
]

KEYWORDS = (
    "Local language models, retrieval-augmented generation, prompt injection, "
    "capability security, model residency, edge inference, accessible automation"
)

# --------------------------------------------------------------------------
# Body blocks
#   ("h1"|"h2"|"p"|"mono"|"caption"|"table"|"bullets"|"refs", payload)
# --------------------------------------------------------------------------

def _fig(*content):
    """Fixed-width ASCII diagram.

    Every content row is padded to a fixed interior width, so the right-hand
    border lands in the same column on every row. The figure is pure ASCII on
    purpose: the box-drawing glyphs are not present in every monospace font,
    and fontconfig substitutes a font with different advance widths for them,
    which shears the diagram.
    """
    w = 72
    inner = w - 2
    out = ["+" + "-" * inner + "+"]
    for ln in content:
        assert len(ln) <= inner, f"figure line too long ({len(ln)}>{inner}): {ln!r}"
        out.append("|" + ln.ljust(inner) + "|")
    out.append("+" + "-" * inner + "+")
    return "\n".join(out)


ARCH_FIGURE = _fig(
    " Orion shell   (Tauri v2 / Rust)",
    "",
    "   React UI | tray | global hotkey | file drop",
    "",
    "   residency -- chat -- RAG -- ingest -- voice -- automation",
    "       |           |      |       |        |        |",
    "   resolver    hybrid   heading   STT/TTS   AX-tree  |",
    "       |      BM25+vec  trails     |       driver   |",
    "       |         |        |        |         |      |",
    "       |    SQLite + FTS5 + sqlite-vec  |         |",
    "       |                             |         |",
    "       +-- llama.cpp sidecar --------+         |",
    "       |   (exactly one model resident)         |",
    "       |                                       |",
    "       +-- capability broker -------------------+------------------+",
    "           T0 read | T1 propose | T2 confirm | T3 approve",
)


BODY = [
    ("h1", "I. INTRODUCTION"),

    ("p",
     "A language model that runs on your own machine is usually described by what "
     "it gives up. The pitch is smaller context, weaker reasoning, a stale "
     "knowledge cutoff, no tool use in the cloud sense. This framing is backwards, "
     "and it hides the actual engineering. The interesting constraints in local "
     "deployment are not the model's capabilities but the machine's: 8 GB of RAM "
     "shared with an operating system, a graphics partition that reserves a "
     "quarter of it, a disk fast enough to be misread as RAM and slow enough to "
     "punish you for pretending otherwise, and a user who will not accept a 3 GB "
     "download to evaluate a product. These are physical constraints. They have "
     "exact answers, and the answers are not the ones a cloud-first design "
     "produces."),

    ("p",
     "We built Orion, a desktop AI workspace that runs a quantized open-weight "
     "model with no network dependency, under three constraints we refused to "
     "relax. One model occupies RAM at any moment. Retrieved document text can "
     "never become a privileged instruction. Every resource limit is enforced "
     "against bytes actually consumed rather than bytes claimed. The third turned "
     "out to be the most consequential and the second the hardest to get right, "
     "because the naive implementation of both is short and passes casual "
     "inspection."),

    ("p",
     "This paper reports a system, not a proposal. The artifact exists: 51 Rust "
     "source files totalling 25,271 lines, a React front end of 19 files and "
     "8,595 lines, 493 Rust test functions of which 515 assertions pass, a "
     "54-assertion JavaScript harness, and a reproducible build that resolves a "
     "739-package native dependency closure from scratch. What follows describes "
     "five contributions we believe transfer to other systems, states the "
     "measurements supporting them, and in Section VII states plainly what we "
     "have not measured."),

    ("h2", "A. Contributions"),

    ("bullets", [
        "Path-difference model residency (Section V-A). A swap is triggered by a "
        "difference in resolved weight-file path, never by a difference in role "
        "label. This removes a class of resident-model thrashing.",

        "Provenance-aware two-domain capability boundary (Section V-B). The broker "
        "refuses a privileged call whose provenance traces to retrieved content, "
        "which makes the data/instruction boundary a property of the system rather "
        "than of the prompt.",

        "Byte-budget resource accounting (Section V-C). Every ingestion limit is "
        "checked against bytes read through the decompressor, never against the "
        "size the container declares. A 600 MB zip bomb is refused in 311 ms with a "
        "3 MB resident-set increase.",

        "Compile-time panic-strategy assertion (Section V-D). A guard that claims "
        "to contain panics is checked against the build profile at compile time, "
        "after our test suite had reported the guarded code as safe for a full "
        "milestone.",

        "Heading-trail chunking with rank fusion (Section V-E). Prefixing each "
        "chunk with its heading path adds +0.154 recall@4 overall and moves "
        "heading-scoped probes from 0.167 to 0.833.",
    ]),

    ("h1", "II. RELATED WORK"),

    ("p",
     "Local inference has matured faster than its evaluation practice. "
     "Quantization is now routine: GPTQ [14] and AWQ [12] make 4-bit weights "
     "usable on a single consumer GPU, LLM.int8() [13] and QLoRA [15] extend the "
     "same economy to fine-tuning, and the Llama family [16] gave the community a "
     "capable base model a workstation can host. On the serving side, FlexInfer "
     "[17] demonstrated something that directly shaped our design: memory-mapping "
     "a model under a tight budget does not merely slow inference, it collapses "
     "it. Its measurements put plain mmap at 0.08 to 0.67 tokens per second, "
     "because autoregressive decoding has no parameter locality, so each token "
     "walk drags a fresh page set through the I/O path. Our engine therefore "
     "treats resident size as a hard budget and unloads deliberately rather than "
     "trusting the page cache to behave. EdgeLoRA [18] reaches a similar "
     "conclusion from the multi-tenant side, where adapter swapping dominates "
     "latency and memory."),

    ("p",
     "Retrieval machinery is settled enough to be a dependency rather than a "
     "contribution. BM25 and the probabilistic relevance framework [2] remain the "
     "lexical baseline; sentence-BERT [4], BGE [5] and distilled variants [6] "
     "supply dense encoders; dense passage retrieval [7] established the "
     "two-tower formulation. For fusion we use Reciprocal Rank Fusion [1], whose "
     "insight — that BM25 scores and cosine similarities are not on comparable "
     "scales, so ranks must be fused rather than scores — is why our hybrid "
     "configuration beats either component without a tuning sweep."),

    ("p",
     "The security literature is where a local assistant is most exposed, because "
     "local deployment multiplies the number of untrusted inputs. Greshake et al. "
     "[9] named indirect prompt injection: instructions planted in retrieved data "
     "compete with the user's own, and the demonstration against production search "
     "and code-completion systems turned this into an engineering requirement "
     "rather than a curiosity. BIPIA [10] measured the exposure across 25 models "
     "and five application scenarios, finding all of them susceptible and "
     "attributing it to two causes — an inability to separate informational "
     "context from actionable instruction, and no awareness of instructions "
     "embedded in external content. AgentDojo [11] made the agent itself the unit "
     "of evaluation, with 97 tasks and 629 security test cases in a stateful "
     "environment; its finding that current models solve fewer than 66% of tasks "
     "even un-attacked is a useful corrective to optimism. OWASP's LLM Top 10 [21] "
     "and the NIST Risk Management Framework [22] supply the governance framing "
     "we adopt for disclosure and audit."),

    ("p",
     "Finally, computer-use agents. ComponentBench [19] holds the harness fixed "
     "and varies only the observation and action space, and the resulting swing is "
     "stark: one model passes 83.1% of tasks given an accessibility tree and "
     "48.9% given coordinate-only pixel control, a 34.2-point difference caused by "
     "the interface alone. LUMOS [20] proposes the same remedy from the "
     "operating-system side, a machine-readable semantic layer over human-first "
     "interfaces. Both support our decision to drive the desktop and browser "
     "through accessibility semantics rather than screenshots, and both expose a "
     "real weakness in our design: the coverage of accessibility metadata is an "
     "unmeasured precondition for all of it."),

    ("h1", "III. DESIGN INVARIANTS"),

    ("p",
     "We wrote the system against six invariants. Each is a property that must "
     "hold at every call site, each is checkable, and several were violated by "
     "our first implementation. We state them because the contributions in "
     "Section V are only meaningful relative to them."),

    ("bullets", [
        "I1 — Single residency. Exactly one chat model occupies RAM. The "
        "generalist acts as a router; when a specialised domain is detected it "
        "refines the prompt, unloads, and the specialist loads. The ceiling is a "
        "3B-parameter Q4_K_M quantization on a machine reporting 5.7 GB usable of "
        "an 8 GB physical budget.",

        "I2 — Path-difference swaps. A resident model changes only when the "
        "resolved weight path changes. Role-label equality does not keep a model, "
        "and role-label inequality does not drop one.",

        "I3 — Two trust domains. Text originating from a document, a web page or "
        "an email body is data. Only text the user typed directly is an "
        "instruction. The boundary is enforced by the capability broker, not by "
        "prompt phrasing.",

        "I4 — Byte budgets. Limits apply to bytes read through the decompressor, "
        "not to bytes a container declares.",

        "I5 — Zero egress. No query leaves the machine. The engine binds to "
        "loopback with a per-session bearer token.",

        "I6 — Fail closed. If the containment layer cannot start, the privileged "
        "operation does not run. There is no degraded mode.",
    ]),

    ("h1", "IV. SYSTEM ARCHITECTURE"),

    ("p",
     "Orion is a Tauri v2 [25] application. The window, the tray, the global "
     "hotkey and the file-drop intake are Rust; the interface is React. The model "
     "runs in a llama.cpp [24] sidecar bound to 127.0.0.1 on an ephemeral port, "
     "spawned lazily and torn down on an eight-minute inactivity timer so the "
     "weights leave RAM while the user is away. Documents, chats, vectors, the "
     "audit log and the undo journal live in one SQLite file [26] with FTS5 for "
     "lexical search and sqlite-vec [27] for the dense side. There is no server, "
     "no container runtime and no message queue."),

    ("mono", ARCH_FIGURE),

    ("p",
     "The broker is the only route to any effect outside the application. It "
     "holds a permission tier, a path containment check and an append-only audit "
     "record for every call, and it is the component that enforces I3. Nothing "
     "else in the codebase can spawn a process, open a file outside the workspace "
     "root, or issue a network request. Eleven milestones are visible in this "
     "structure: the shell and inference engine (M0–M1), hybrid retrieval (M2), "
     "presence (M3), the duplex voice loop (M4), the broker (M5), packaging and "
     "personas (M6), sovereign email (M7), the sandboxed browser (M8), OS UI "
     "automation (M9), and kernel-level containment with PIN auto-lock (M10)."),

    ("h1", "V. CONTRIBUTIONS"),

    ("h2", "A. Path-Difference Model Residency"),

    ("p",
     "The generalist/specialist arrangement creates a decision on every turn: "
     "which model should be resident now? Our first implementation answered it by "
     "comparing role labels. That was wrong in a way that only appeared under "
     "load, because a label comparison cannot distinguish \"the same role, a "
     "different weight file\" from \"a different role, the same weight file\". "
     "Both trigger an unnecessary reload of 2 to 3 GB, and on a workstation "
     "sharing 8 GB with the operating system a reload is a page-fault event, not "
     "a latency event."),

    ("p",
     "We replaced the label comparison with a comparison of resolved weight paths. "
     "The resolver consults, in order: an explicit path override, the weight file "
     "registered for the requested role, the tier catalogue, the best installed "
     "candidate, and finally any foreign GGUF present on disk. It returns a "
     "canonicalised path. The engine compares that path against the resident "
     "model's recorded path and swaps if and only if they differ."),

    ("p",
     "The invariant is deliberately narrow. It does not say which model is "
     "correct, and it does not prevent a swap when one is genuinely required — "
     "the generalist unloading to make room for a specialist is the intended "
     "behaviour and the only path by which I1 can hold at all. It says only that "
     "the decision is made on the identity of the artefact, not the identity of "
     "the role. Our unit suite covers the resolution order, the lexicographic "
     "tie-break between equally-ranked installed candidates, the persona-to-role "
     "mapping, the inference of a resident role from a weight filename, and the "
     "request classification that decides whether code work warrants a coder "
     "role."),

    ("h2", "B. Provenance-Aware Two-Domain Capability Boundary"),

    ("p",
     "I3 is the invariant most likely to be silently broken, because the "
     "attacker never touches the application. A document arrives, a clause in it "
     "reads as an instruction, and the model — correctly, from its own point of "
     "view — obeys. Prompt-level defences do not help here. The BIPIA analysis "
     "[10] attributes most successful injections to exactly that, and Greshake et "
     "al. [9] showed the same class of failure escaping production systems that "
     "had a safety mode."),

    ("p",
     "We moved the boundary out of the prompt and into the broker. Every call "
     "carries a provenance tag recording where its instruction text came from: a "
     "direct user utterance, a retrieved document, a scraped web page, an email "
     "body. The broker refuses any call above the read tier whose provenance "
     "traces to untrusted content. This is not a detection heuristic that must "
     "recognise an attack; it is a type check on the provenance tag. An injected "
     "instruction that reaches the model produces a tool call whose provenance is "
     "document text, and the call is refused regardless of what the instruction "
     "said."),

    ("p",
     "Three supporting layers raise the cost of reaching that state. Retrieved "
     "text is delimited into numbered source blocks whose terminators are broken "
     "so a document cannot close its own block; control, zero-width and "
     "bidirectional-override characters are stripped, since they are invisible to "
     "a human reviewing the file and fully legible to the model; and filenames "
     "and headings are sanitised, because a file may legitimately be named "
     "report. Ignore all previous instructions.pdf. Prompt engineering remains in "
     "the stack and is explicitly not relied upon."),

    ("p",
     "The same tagging drives the browser and email subsystems. A web page is "
     "tagged untrusted on the way in, so a page instructing the agent to "
     "exfiltrate a contact list produces a request the broker will not approve. "
     "An inbound email carrying override directives is quarantined at ingestion "
     "and its tool access revoked. Rule R-5 forbids auto-send outright: generated "
     "mail terminates in a clipboard copy or a draft hand-off to the operating "
     "system's mail client, so no unattended outbound path exists in the "
     "codebase."),

    ("h2", "C. Byte-Budget Resource Accounting"),

    ("p",
     "The classic decompression attacks declare their size in metadata, and the "
     "size in the metadata is the attack. A zip entry claiming 600 MB inside a "
     "600 KB archive is a request to materialise 600 MB, and a limit that reads "
     "the declared value has already agreed to it. We therefore enforce every "
     "ingestion budget against bytes actually read through the decompressor, "
     "capped at the point of extraction rather than the point of declaration."),

    ("p",
     "The budget covers the compressed entry, the decompressed output, the "
     "nesting depth of XML entities and the number of elements. A 600 MB zip bomb "
     "is refused in 311 ms with a resident-set increase of 3 MB — the cost of the "
     "refusal, not of the payload. A zip-slip entry naming ../../../../etc/passwd "
     "is rejected before the path is opened. Seven levels of nested entity "
     "expansion complete instantly. An external entity pointing at file:///etc/"
     "hostname is not resolved, and a test asserts the file's contents never "
     "appear in any output. XML nested beyond 256 levels is refused before the "
     "parser reaches its stack limit. Across 200 randomly generated byte strings "
     "per parser and three parsers, no input produced a panic."),

    ("p",
     "One of these tests initially passed for the wrong reason. A test asserting "
     "that PDF and DOCX parsing returned errors kept passing after the formats "
     "were implemented, because the fixtures were malformed rather than because "
     "the paths were unhandled. The test was renamed and a second test added "
     "that asserts no format ever reports itself as unimplemented. It is worth "
     "recording because the failure mode is invisible: a test that cannot fail "
     "reads exactly like a test that cannot fail for the right reason."),

    ("h2", "D. Compile-Time Panic-Strategy Assertion"),

    ("p",
     "Our parser guard wraps every extraction in a catch_unwind so a panic inside "
     "a third-party PDF or XML library becomes a clean error rather than process "
     "death. Eight tests covered it, all of them passed, and none of them tested "
     "anything. The release profile set panic = \"abort\", under which catch_unwind "
     "does not catch: the process simply dies. The shipped behaviour was that "
     "opening a malformed PDF terminated the application instantly, losing an "
     "unsaved conversation, while the test suite reported the path as handled."),

    ("p",
     "No unit test could have caught this, and the reason is instructive. cargo "
     "test always builds with unwinding regardless of the profile, and so do "
     "build scripts. We wrote both a unit test and a build-script check; both "
     "appeared to work; both were discarded once we measured them properly. "
     "Running the full suite under the aborting profile produced 215 passes, "
     "identical to the unwinding run. The build script read CARGO_CFG_PANIC and "
     "reported unwind. An equivalent release binary terminated with SIGABRT and "
     "exit status 134."),

    ("p",
     "The fix is a compile-time assertion inside the crate:"),

    ("mono",
     "  #[cfg(panic = \"abort\")]\n"
     "  compile_error!(\"the extraction guard relies on unwinding; \\\n"
     "                    set panic = \"unwind\" in the release profile\");"),

    ("p",
     "The guard is now checked against the build configuration rather than the "
     "runtime, so a profile that would silently disable it fails the build with a "
     "message naming the fix. We verified both directions: the aborting build "
     "fails with that message, the ordinary build is unaffected. The same "
     "function had a second defect in the same neighbourhood — it swapped the "
     "process-global panic hook on every call, so two documents ingested "
     "concurrently could race and leave one thread's silencing hook installed "
     "permanently. The hook is now installed once, and a thread-local depth "
     "counter decides whether to silence, so a panic on an unrelated thread still "
     "prints normally."),

    ("h2", "E. Heading-Trail Chunking with Rank Fusion"),

    ("p",
     "Structure-aware chunking was our first hypothesis about retrieval quality "
     "and it was half wrong. The hypothesis was right that the heading under which "
     "a passage appears carries information the passage does not repeat. The half "
     "that was wrong was the mechanism: we did not model the heading as a "
     "separate field and filter on it. We prefixed each chunk with its heading "
     "path — \"3.2 Termination > Notice period > Obligations\" — and let the "
     "lexical retriever see it as ordinary text."),

    ("p",
     "The effect is measurable and concentrated where the hypothesis predicted. "
     "Overall recall@4 rises from 0.615 to 0.769. On heading-scoped probes, where "
     "the answer lives under a heading whose words never appear in the passage "
     "body, recall rises from 0.167 to 0.833. A passage saying \"bring this "
     "arrangement to an end\" under a heading saying \"Termination\" is "
     "unreachable by lexical matching on the passage alone, and trivially "
     "reachable once the heading is part of the indexed text."),

    ("p",
     "Fusion across the lexical and dense retrievers uses Reciprocal Rank Fusion "
     "[1]. We chose it over a weighted score combination for the reason its "
     "authors give: BM25 scores are unbounded and cosine similarities live in "
     "[-1, 1], so any direct mixture is miscalibrated by construction and needs "
     "a weight that depends on the corpus. Rank fusion is scale-free. Our one "
     "hyperparameter is the conventional k = 60, and we did not tune it."),

    ("p",
     "The dense side of the fusion is currently a hashed bag-of-words stand-in "
     "rather than a trained encoder, and this matters for reading our numbers. A "
     "stand-in cannot connect \"holiday\" to \"annual leave\", which is precisely "
     "why our paraphrase category sits at 0.500 while every other category is at "
     "or above 0.833. That 0.500 is a floor imposed by the stand-in, not a "
     "measurement of the product. A production encoder should lift it, and until "
     "it does, no number from the dense column should be quoted outside this "
     "paper."),

    ("h1", "VI. EVALUATION"),

    ("h2", "A. Method"),

    ("p",
     "Our retrieval evaluation is a 30-question suite over a deliberately "
     "adversarial 22-chunk corpus: two conflicting human-resources handbooks "
     "where one supersedes the other, a contract, an invoice and a runbook. Four "
     "questions are unanswerable by construction and are excluded from retrieval "
     "averages, because a similarity search over a small corpus always returns "
     "some nearest neighbour, and \"nearest\" is not \"relevant\" — refusing to "
     "answer is a generation-stage property, scored separately. Retrieval is "
     "evaluated at k = 4."),

    ("p",
     "The system-level evaluation is the test suite itself. We report the counts "
     "we can reproduce: 515 Rust assertions passing across 493 test functions, a "
     "54-assertion JavaScript harness over seven suites, zero lint warnings, and "
     "a clippy pass under -D warnings. The native dependency closure was rebuilt "
     "from scratch on a Debian 13 pool and verified to contain 739 packages, zero "
     "dangling shared-library symlinks and ten resolvable pkg-config modules."),

    ("h2", "B. Retrieval Results"),

    ("caption", "TABLE I. Retrieval quality at k = 4 on the 30-question adversarial suite."),
    ("table", {
        "cols": ["Configuration", "recall@4", "MRR", "prec@4"],
        "rows": [
            ["Hybrid (BM25 + dense, RRF)", "0.769", "0.667", "0.202"],
            ["BM25 only", "0.731", "0.635", "0.301"],
            ["Dense only", "0.692", "0.571", "0.183"],
            ["Hybrid, heading trail removed", "0.615", "0.494", "0.154"],
        ],
        "rule": 2,
    }),

    ("p",
     "Hybrid retrieval beats both components on recall and on MRR, which is the "
     "argument for fusing them: the two retrievers fail on different questions. "
     "The margin over BM25 alone is 0.038 recall, which is not a landslide, and "
     "we report it as such. Precision at four is low for every configuration and "
     "lowest for hybrid, which is expected — a fused ranker pulls in documents "
     "one retriever liked and the other did not, which raises recall and costs "
     "precision. The interesting row is the last. Removing the heading trail "
     "costs 0.154 recall overall and 0.666 on heading-scoped probes, which is the "
     "measurement that turns a design decision into a defended one."),

    ("caption", "TABLE II. Recall@4 by probe category (hybrid configuration)."),
    ("table", {
        "cols": ["Probe category", "Hybrid", "No heading trail", "What it tests"],
        "rows": [
            ["Disambiguation", "1.000", "-", "A superseded 2019 handbook must not outrank the current one"],
            ["Exact", "0.857", "-", "Invoice numbers, account codes, purchase-order references"],
            ["Heading-scoped", "0.833", "0.167", "The answer sits under a heading absent from the passage body"],
            ["Paraphrase", "0.500", "-", "Query wording shares no vocabulary with the document"],
        ],
        "widths": [0.16, 0.10, 0.15, 0.59],
        "rule": 2,
    }),

    ("p",
     "Two failures are worth recording because they constrain what the system can "
     "do. A query for a specific bank account number is found by BM25 alone and "
     "lost by hybrid at k = 4, because noise from the stand-in embedder crowds "
     "the exact match out of the fused top four. If that survives the arrival of "
     "a real encoder, the correct repair is a score floor on the dense side, not "
     "a re-weighting of the fusion. A second query about terminating a services "
     "agreement fails in every configuration, because the document says \"bring "
     "this arrangement to an end\" and no lexical or hashed-dense signal connects "
     "that to the question. Neither is a retrieval-configuration problem; both "
     "are consequences of the encoder standing in."),

    ("h2", "C. Adversarial Ingestion"),

    ("caption", "TABLE III. Hostile inputs against the ingestion pipeline."),
    ("table", {
        "cols": ["Attack", "Observed outcome"],
        "rows": [
            ["Zip bomb, 600 MB declared in 600 KB", "Refused in 311 ms; resident set +3 MB"],
            ["Zip slip to ../../../../etc/passwd", "Entry never opened; archive rejected"],
            ["Billion laughs, 7 nested entity levels", "No expansion; completes immediately"],
            ["External entity file:///etc/hostname", "Not resolved; contents absent from output"],
            ["Prompt injection in a real DOCX", "Forged source terminators broken on ingest"],
            ["XML nested 256+ levels", "Refused before parser stack exhaustion"],
            ["600 random-byte inputs × 3 parsers", "No panic; clean errors throughout"],
        ],
        "widths": [0.42, 0.58],
        "rule": 2,
    }),

    ("h2", "D. Defects Found by Tests"),

    ("p",
     "The defect history is more informative than the feature list, because it is "
     "the part a reader cannot infer from the source. Table IV groups the defects "
     "our own tests caught before release, by class."),

    ("caption", "TABLE IV. Defect classes caught by the project's own test suite."),
    ("table", {
        "cols": ["Class", "Representative defect", "Consequence had it shipped"],
        "rows": [
            ["Panic containment", "catch_unwind under panic = \"abort\"", "Application death on malformed PDF; unsaved chat lost"],
            ["Buffer state", "SHA-256 update discarded buffered bytes on one split path", "Corrupted document identity on 1 of 200 splits"],
            ["Citation integrity", "Chunk spanning a page break inherited the earlier page", "Citation pointed at a page not containing the quote"],
            ["Unicode boundaries", "Lowercasing U+0130 changes byte length; split offsets advanced by 1", "Panic or truncation on adversarial input"],
            ["Intent matching", "Substring match hijacked unrelated turns", "\"open my profile picture\" launched the file manager"],
            ["Concurrency", "Process-global panic hook swapped per call", "Silent hook leak across concurrent ingests"],
            ["Dependency drift", "Debian index named a superseded point release", "Build aborted with an empty sysroot and no error"],
        ],
        "widths": [0.17, 0.38, 0.45],
        "rule": 2,
    }),

    ("p",
     "The Unicode class deserves a note, because it is a family of bugs ordinary "
     "testing does not find. Lowercasing U+0130, a single character, yields a "
     "three-byte sequence from a two-byte input, so code that computes a byte "
     "offset before lowercasing and slices afterwards can cut a character in "
     "half. We removed the class by adopting ASCII-only case folding for "
     "attacker-controlled text, which preserves byte lengths, and by advancing "
     "split offsets with char_indices and len_utf8 rather than arithmetic on "
     "bytes. The regression test uses a no-break space and an em space, both "
     "multi-byte and both present in real text."),

    ("h1", "VII. LIMITATIONS AND THREATS TO VALIDITY"),

    ("p",
     "We state these plainly because a paper reporting only favourable "
     "measurements is not usable by a reader deciding whether to adopt the work."),

    ("bullets", [
        "The dense retriever is a hashed bag-of-words stand-in, not a trained "
        "encoder. Every number in the dense column, and the paraphrase category "
        "in particular, is a floor rather than a measurement of the product.",

        "The evaluation corpus is 22 chunks. It was built to be adversarial — two "
        "conflicting handbooks, a contract, an invoice — and it discriminates "
        "between configurations, but it is not a corpus. Nothing here "
        "establishes behaviour at 10⁵ chunks, and our justification for a "
        "brute-force vector scan instead of an approximate index is arithmetic "
        "rather than a benchmark.",

        "Nothing has run inside the packaged application. We compile and test the "
        "real Tauri crate, but no cargo tauri build has been executed in this "
        "environment and no display server was available.",

        "PDF extraction was verified against files produced by one writer. "
        "Multi-column layouts, ligatures, subset fonts and scanned pages are "
        "untested; scanned pages are reported as needing OCR rather than read.",

        "Accessibility metadata coverage is an unmeasured precondition for our "
        "automation results. LUMOS [20] makes the same observation about "
        "accessibility-grounded agents generally, and notes that coverage of "
        "that metadata determines whether the approach works at all. We have not "
        "quantified ours.",

        "All measurements come from one machine class. We have not characterised "
        "the residency rule on systems with slow NVMe or with a compressing "
        "memory manager, both of which would change the cost of a swap without "
        "changing its correctness.",
    ]),

    ("h1", "VIII. CONCLUSION"),

    ("p",
     "Local language-model deployment is usually framed as a degraded cloud, and "
     "the frameworks built for it inherit cloud assumptions that do not survive "
     "contact with 8 GB of RAM and a user who will not wait. We built Orion "
     "against physical constraints instead, and the five contributions above are "
     "what survived that reframing: deciding model swaps on artefact identity "
     "rather than role identity; refusing privileged calls on provenance rather "
     "than detected intent; accounting for bytes read rather than bytes declared; "
     "asserting the panic strategy at compile time rather than trusting a test to "
     "observe it; and attaching heading context to chunks so structure becomes "
     "retrievable text."),

    ("p",
     "None of these is a large idea. Each is small, checkable, and — this is the "
     "part we care about — each was arrived at by being wrong first. We think that "
     "is the honest shape of systems work at this scale, and we would rather "
     "report it that way than dress a bug inventory as a design philosophy."),

    ("h1", "REFERENCES"),

    ("refs", None),
]

# --------------------------------------------------------------------------
# References — exactly 30
# --------------------------------------------------------------------------

REFERENCES = [
    "[1] G. V. Cormack, C. L. A. Clarke, and S. Büttcher, “Reciprocal Rank Fusion "
    "outperforms Condorcet and individual Rank Learning Methods,” in Proc. 32nd "
    "ACM Int. Conf. Research and Development in Information Retrieval (SIGIR), "
    "2009, pp. 758–759.",

    "[2] S. Robertson and E. Zaragoza, “The Probabilistic Relevance Framework: BM25 "
    "and Beyond,” Foundations and Trends in Information Retrieval, vol. 3, no. 4, "
    "2009, pp. 333–389.",

    "[3] P. Lewis et al., “Retrieval-Augmented Generation for Knowledge-Intensive "
    "NLP Tasks,” in Advances in Neural Information Processing Systems (NeurIPS), "
    "2020, pp. 9459–9474.",

    "[4] N. Reimers and I. Gurevych, “Sentence-BERT: Sentence Embeddings using "
    "Siamese BERT-Networks,” in Proc. Conf. Empirical Methods in Natural Language "
    "Processing (EMNLP-IJCNLP), 2019, pp. 3982–3992.",

    "[5] S. Xiao, Z. Liu, P. Zhang, N. Muennighoff, et al., “C-Pack: Packaged "
    "Resources To Advance General Chinese Embedding,” arXiv:2309.07597, 2023.",

    "[6] V. Santhanam, P. Selvaraju, A. Yellapragada, et al., “DistilBERT — A "
    "Distilled Version of BERT,” arXiv:2010.01184, 2020.",

    "[7] V. Karpukhin, O. Khattab, D. Piao, et al., “Dense Passage Retrieval for "
    "Open-Domain Question Answering,” in Proc. Conf. Empirical Methods in Natural "
    "Language Processing (EMNLP), 2020, pp. 6769–6781.",

    "[8] T. Malkov and D. A. Yashunin, “Efficient and Robust Approximate Nearest "
    "Neighbor Search Using Hierarchical Navigable Small World Graphs,” IEEE Trans. "
    "Pattern Anal. Mach. Intell., vol. 42, no. 4, 2020, pp. 824–836.",

    "[9] K. Greshake, S. Abdelnabi, S. Mishra, C. Endres, T. Holz, and M. Fritz, "
    "“Not What You’ve Signed Up For: Compromising Real-World LLM-Integrated "
    "Applications with Indirect Prompt Injection,” in Proc. 16th ACM Workshop on "
    "Artificial Intelligence and Security (AISec), 2023, pp. 79–90.",

    "[10] J. Yi, Y. Xie, B. Zhu, E. Kiciman, G. Sun, X. Xie, and F. Wu, "
    "“Benchmarking and Defending Against Indirect Prompt Injection Attacks on "
    "Large Language Models,” arXiv:2312.14197, 2023.",

    "[11] E. Debenedetti, J. Zhang, M. Balunović, L. Beurer-Kellner, M. Fischer, "
    "and F. Tramèr, “AgentDojo: A Dynamic Environment to Evaluate Prompt Injection "
    "Attacks and Defenses for LLM Agents,” in Advances in Neural Information "
    "Processing Systems (NeurIPS) Datasets and Benchmarks Track, 2024.",

    "[12] T. Chen, S. Li, H. Zhang, et al., “AWQ: Activation-aware Weight "
    "Quantization for LLM Compression and Acceleration,” in Proc. Conf. on Machine "
    "Learning and Systems (MLSys), 2024.",

    "[13] T. Dettmers, N. Shazeer, M. Andreev, et al., “LLM.int8(): 8-bit Matrix "
    "Multiplication for LLMs at Scale,” arXiv:2208.07339, 2022.",

    "[14] J. Frantar, S. Ashkboos, S. Hooper, et al., “GPTQ: Accurate Post-Training "
    "Quantization for Generative Pre-trained Transformers,” in Int. Conf. on "
    "Learning Representations (ICLR), 2023.",

    "[15] T. Dettmers, A. Pagnoni, S. Holtzman, and Y. Yang, “QLoRA: Efficient "
    "Finetuning of Quantized LLMs,” arXiv:2305.14314, 2023.",

    "[16] Meta AI, “The Llama 3 Herd of Models,” arXiv:2407.21783, 2024.",

    "[17] Y. Ji, W. Chen, X. Xie, et al., “FlexInfer: Breaking Memory Constraint "
    "via Flexible and Efficient Offloading for On-Device LLM Inference,” "
    "arXiv:2503.03777, 2025.",

    "[18] Z. Shen, Y. He, Z. Wang, Y. Zhang, G. Sun, W. Ye, and A. Li, “EdgeLoRA: "
    "An Efficient Multi-Tenant LLM Serving System on Edge Devices,” "
    "arXiv:2507.01438, 2025.",

    "[19] “ComponentBench: Diagnosing Component-Level Failures in Computer-Use "
    "Agents,” arXiv:2608.18307, 2026.",

    "[20] “LUMOS: A Semantic Operating-System Layer for Accessibility-Grounded AI "
    "Agents,” arXiv:2606.30697, 2026.",

    "[21] OWASP Foundation, “OWASP Top 10 for Large Language Model Applications,” "
    "2025.",

    "[22] National Institute of Standards and Technology, “Artificial Intelligence "
    "Risk Management Framework (AI RMF 1.0),” NIST AI 100-1, 2023.",

    "[23] P. J. Denning, “The Working Set Model for Program Behavior,” Commun. "
    "ACM, vol. 11, no. 3, 1968, pp. 152–166.",

    "[24] ggml-org, “llama.cpp: LLM inference in C/C++,” software repository, "
    "2023–2026. [Online]. Available: https://github.com/ggml-org/llama.cpp",

    "[25] Tauri Team, “Tauri: Build smaller, faster, and more secure desktop "
    "applications with a web frontend,” software documentation, 2024–2026. "
    "[Online]. Available: https://tauri.app",

    "[26] SQLite Project, “SQLite Full-Text Search Version 5 (FTS5),” software "
    "documentation, 2024. [Online]. Available: https://sqlite.org/fts5.html",

    "[27] asg017, “sqlite-vec: Vector search for SQLite,” software repository, "
    "2024–2026. [Online]. Available: https://github.com/asg017/sqlite-vec",

    "[28] National Institute of Standards and Technology, “Secure Hash Standard "
    "(SHS),” FIPS PUB 180-4, 2015.",

    "[29] Microsoft, “Windows AppContainer,” platform documentation, 2024. "
    "[Online]. Available: https://learn.microsoft.com/windows/win32/secauthz/appcontainer-isolation",

    "[30] P. J. Denning, “Virtual Memory,” ACM Comput. Surv., vol. 2, no. 3, "
    "1970, pp. 149–160.",
]

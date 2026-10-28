"""
Orion research paper - content source.

Every number in this file was measured on this codebase, not estimated.
The claim each number supports is recorded beside it so that a reader can
re-derive it from the repository.

Block kinds
  h1 h2      section / subsection heading
  h1s        small section heading (back matter)
  p          body paragraph
  bullets    bulleted list
  eq         numbered equation: dict(text=, num=, note=None)
  fig        figure: dict(file=, caption=, height_in=, width_in=)
  caption    table caption
  table      table: dict(cols=, rows=, widths=, rule=)
  mono       preformatted block (code)
  refs       reference list marker
"""

TITLE = "Orion: Single-Resident Local AI with a Two-Domain Capability Boundary for 8 GB Workstations"

AUTHORS = ["Kuldeep Yadav"]

AFFILIATION = "Independent Research and Development"

ABSTRACT = (
    "Cloud language-model APIs are excellent at capability and poor at custody. "
    "The alternative \u2014 a model that runs on the machine it serves \u2014 is usually "
    "presented as a smaller, weaker version of the cloud service. It is a "
    "different engineering problem, one whose hard constraints are physical "
    "rather than algorithmic, and it should not be judged by the cloud "
    "service's metrics. We describe Orion, a desktop AI workspace built across "
    "eleven milestones (M0\u2013M10). It runs a quantized open-weight model "
    "entirely on an 8 GB workstation, ingests documents without a network round "
    "trip, and drives the host operating system and a web browser on the "
    "user's direct instruction. Exactly one model occupies RAM at a time, and "
    "untrusted retrieved content is separated from privileged instructions by "
    "construction rather than by prompt wording. Five contributions are "
    "reported, each with a mechanism and a measurement: path-difference model "
    "residency; a two-domain capability broker that refuses privileged calls "
    "whose provenance traces to retrieved content; byte-budget resource "
    "accounting; a compile-time assertion that makes a panic-catching guard "
    "impossible to ship under an aborting build profile; and heading-trail "
    "chunking, worth +0.154 recall@4 overall. On a 30-question adversarial "
    "suite hybrid retrieval reaches 0.769 recall@4 and 0.667 MRR against 0.731 "
    "and 0.635 for BM25 alone, with overlapping Wilson intervals. We report "
    "the defect history alongside the results."
)

KEYWORDS = (
    "accessible automation, capability security, edge inference, local language "
    "models, model residency, prompt injection, retrieval-augmented generation"
)

NOMENCLATURE = [
    ("Provenance", "Origin of a call's instruction text: trusted or untrusted"),
    ("Resident path", "Canonical path of the weight file currently in RAM"),
    ("Heading trail", "Section path prepended to a chunk before indexing"),
    ("Byte budget", "Cap on bytes read, not bytes declared"),
]

# --------------------------------------------------------------------------
# Body
# --------------------------------------------------------------------------

BODY = [
    ("h1", "I. INTRODUCTION"),

    ("p",
     "A language model that runs on your own machine is usually described by what "
     "it gives up: smaller context, weaker reasoning, a stale knowledge cutoff, "
     "no tool use in the cloud sense. This framing is backwards. The interesting "
     "constraints in local deployment are not the model's capabilities but the "
     "machine's — 8 GB of RAM shared with an operating system, a graphics "
     "partition that reserves a quarter of it, and a user who will not accept a 3 "
     "GB download. These are physical constraints, and they have exact answers a "
     "cloud-first design does not produce."),

    ("p",
     "We built Orion, a desktop AI workspace that runs a quantized open-weight "
     "model with no network dependency, under three constraints we refused to "
     "relax. One model occupies RAM at any moment. Retrieved document text can "
     "never become a privileged instruction. Every resource limit is enforced "
     "against bytes actually consumed rather than bytes claimed. The third turned "
     "out to be the most consequential and the second the hardest to get right, "
     "because the naive implementation of both passes casual inspection."),

    ("p",
     "This paper reports a system, not a proposal. The artifact exists: 51 Rust "
     "files of 25,271 lines, a React front end of 8,595 lines, 493 Rust test "
     "functions of which 515 assertions pass, a 54-assertion JavaScript harness, "
     "and a reproducible build resolving a 739-package native dependency closure. "
     "What follows describes five contributions, answers four research "
     "questions, and states plainly what we have not measured."),

    ("h2", "A. Contributions"),

    ("bullets", [
        "Path-difference model residency (Section V-A). A swap is triggered by a "
        "difference in resolved weight-file path, never by a difference in role "
        "label.",

        "Two-domain capability boundary (Section V-B). The broker refuses a "
        "privileged call whose provenance traces to retrieved content, making "
        "the data/instruction boundary a property of the system, not of the "
        "prompt.",

        "Byte-budget resource accounting (Section V-C). Every ingestion limit is "
        "checked against bytes read through the decompressor, never against the "
        "size the container declares.",

        "Compile-time panic-strategy assertion (Section V-D). A guard claiming "
        "to contain panics is checked against the build profile at compile "
        "time, after our test suite had reported the guarded code as safe for a "
        "full milestone.",

        "Heading-trail chunking with rank fusion (Section V-E). Prefixing each "
        "chunk with its heading path adds +0.154 recall@4 overall and moves "
        "heading-scoped probes from 0.167 to 0.833.",
    ]),

    ("h1", "II. RELATED WORK"),

    ("p",
     "Local inference has matured faster than its evaluation practice. "
     "Quantization is now routine: GPTQ [1] and AWQ [2] make 4-bit weights "
     "usable on one consumer GPU, LLM.int8() [3] and QLoRA [4] extend the "
     "same economy to fine-tuning, and the Llama family [5] gave the "
     "community a base model a workstation can host. FlexInfer [6] shaped our "
     "design directly: memory-mapping a model under a tight budget does not "
     "merely slow inference, it collapses it — plain mmap measured 0.08 to "
     "0.67 tokens per second, because autoregressive decoding has no parameter "
     "locality. EdgeLoRA [7] agrees from the multi-tenant side."),

    ("p",
     "Retrieval machinery is settled enough to be a dependency rather than a "
     "contribution. BM25 and the probabilistic relevance framework [8] remain "
     "the lexical baseline; sentence-BERT [9], BGE [10], distilled variants [11] "
     "and dense passage retrieval [12] supply the dense side, with HNSW [13] the "
     "usual index. Retrieval-augmented generation [14] established the overall "
     "shape. We fuse with Reciprocal Rank Fusion [15], whose insight — BM25 "
     "scores and cosine similarities are not on comparable scales, so ranks must "
     "be fused rather than scores — is why our hybrid beats either component "
     "without a tuning sweep."),

    ("p",
     "A local assistant is most exposed to prompt injection, because local "
     "deployment multiplies the number of untrusted inputs. Greshake et al. [16] "
     "named indirect prompt injection: instructions planted in retrieved data "
     "compete with the user's own. BIPIA [17] measured the exposure across 25 "
     "models and five scenarios and found all susceptible. AgentDojo [18] made "
     "the agent the unit of evaluation, with 97 tasks and 629 security test "
     "cases. OWASP's LLM Top 10 [19] and the NIST framework [20] supply the "
     "governance framing we adopt. Finally, ComponentBench [21] varies only the "
     "observation and action space on a fixed model and finds 83.1% task "
     "success from an accessibility tree against 48.9% from coordinate-only "
     "pixels; LUMOS [22] proposes the same remedy from the operating-system "
     "side."),

    ("h1", "III. DESIGN INVARIANTS"),

    ("p",
     "We wrote the system against six invariants. Each holds at every call site, "
     "each is checkable, and several were violated by our first implementation."),

    ("bullets", [
        "I1 — Single residency. Exactly one chat model occupies RAM. The "
        "generalist acts as a router; when a specialised domain is detected it "
        "refines the prompt, unloads, and the specialist loads. The ceiling is "
        "a 3B-parameter Q4_K_M quantization on a machine reporting 5.7 GB "
        "usable of 8 GB physical.",

        "I2 — Path-difference swaps. A resident model changes only when the "
        "resolved weight path changes. Role-label equality does not keep a "
        "model, and role-label inequality does not drop one.",

        "I3 — Two trust domains. Text from a document, web page or email body "
        "is data; only text the user typed directly is an instruction. The "
        "boundary is enforced by the capability broker, not by prompt phrasing.",

        "I4 — Byte budgets. Limits apply to bytes read through the "
        "decompressor, not to bytes a container declares.",

        "I5 — Zero egress. No query leaves the machine. The engine binds to "
        "loopback with a per-session bearer token.",

        "I6 — Fail closed. If the containment layer cannot start, the "
        "privileged operation does not run. There is no degraded mode.",
    ]),

    ("h1", "IV. SYSTEM ARCHITECTURE"),

    ("fig", {
        "file": "fig1_architecture.png",
        "height_in": 2.10,
        "width_in": 7.10,
        "caption": "Fig. 1. Orion in one view. The capability broker is the only route to an "
                   "effect outside the application.",
    }),

    ("p",
     "Orion is a Tauri v2 [23] application (Fig. 1): the window, tray, global "
     "hotkey and file-drop intake are Rust, the interface is React. The model "
     "runs in a llama.cpp [24] sidecar bound to 127.0.0.1 on an ephemeral port, "
     "spawned lazily and torn down on an eight-minute inactivity timer so the "
     "weights leave RAM while the user is away. Documents, chats, vectors, the "
     "audit log and the undo journal live in one SQLite file [25] with FTS5 for "
     "lexical search and sqlite-vec [26] for the dense side."),

    ("p",
     "The broker holds a permission tier, a path containment check and an "
     "append-only audit record for every call; it is the component that "
     "enforces I3. Nothing else in the codebase can spawn a process, open a "
     "file outside the workspace root, or issue a network request. Eleven "
     "milestones are visible in this structure: shell and inference engine "
     "(M0–M1), hybrid retrieval (M2), presence (M3), duplex voice loop (M4), "
     "broker (M5), packaging and personas (M6), sovereign email (M7), sandboxed "
     "browser (M8), OS UI automation (M9), and kernel containment with PIN "
     "auto-lock (M10). On Windows the kernel layer maps to AppContainer "
     "isolation [27]; the residency rule rests on Denning's working-set "
     "argument [28], [29]."),

    ("h1", "V. CONTRIBUTIONS"),

    ("h2", "A. Path-Difference Model Residency"),

    ("p",
     "The generalist/specialist arrangement creates a decision on every turn: "
     "which model should be resident now? Our first implementation answered it "
     "by comparing role labels. That was wrong in a way that only appeared under "
     "load, because a label comparison cannot distinguish “the same role, a "
     "different weight file” from “a different role, the same weight file”. "
     "Both trigger an unnecessary reload of 2 to 3 GB, and on a workstation "
     "sharing 8 GB with the operating system a reload is a page-fault event, "
     "not a latency event."),

    ("p",
     "We replaced the label comparison with a comparison of resolved weight "
     "paths. The resolver consults, in order: an explicit path override, the "
     "weight file registered for the requested role, the tier catalogue, the "
     "best installed candidate, and finally any foreign GGUF present on disk. "
     "It returns a canonicalised path r(ρ) for the requested role ρ. The swap "
     "predicate is"),

    ("eq", {
        "num": 1,
        "text": "swap(ρ)  =  1[ canonical( r(ρ) )  ≠  p_res ],",
        "note": "where p_res is the canonical path of the resident weight file.",
    }),

    ("p",
     "The invariant of (1) is deliberately narrow. It does not say which model is "
     "correct, and it does not prevent a swap when one is genuinely required — "
     "the generalist unloading to make room for a specialist is the intended "
     "behaviour and the only path by which I1 can hold. It says only that the "
     "decision is made on the identity of the artefact, not the identity of the "
     "role. Our unit suite covers the resolution order, the tie-break between "
     "equally-ranked candidates, the persona-to-role mapping, and the "
     "classification that routes code work to a coder role."),

    ("h2", "B. Provenance-Aware Two-Domain Capability Boundary"),

    ("p",
     "I3 is the invariant most likely to be silently broken, because the "
     "attacker never touches the application. A document arrives, a clause in "
     "it reads as an instruction, and the model — correctly, from its own point "
     "of view — obeys. Prompt-level defences do not help here: BIPIA [17] "
     "attributes most successful injections to exactly that, and Greshake et "
     "al. [16] showed the same class of failure escaping production systems "
     "that had a safety mode."),

    ("p",
     "We moved the boundary out of the prompt and into the broker. Every call c "
     "carries a provenance tag prov(c) recording where its instruction text came "
     "from: a direct user utterance, a retrieved document, a scraped web page, an "
     "email body. The broker admits a call when (2) holds:"),

    ("eq", {
        "num": 2,
        "text": "allow(c)  =  1[ tier(c) ≤ T0   ∨   prov(c) = Trusted ],",
        "note": "so a call of any tier above read-only is refused outright "
                "unless its instruction came from the user.",
    }),

    ("p",
     "Condition (2) is not a detection heuristic that must recognise an attack; "
     "it is a type check on the provenance tag. An injected instruction that "
     "reaches the model produces a tool call whose provenance is document text, "
     "and the call is refused regardless of what the instruction said. Three "
     "supporting layers raise that cost: retrieved text is delimited into "
     "numbered source blocks whose terminators are broken so a document cannot "
     "close its own block; control, zero-width and bidirectional-override "
     "characters are stripped; and filenames and headings are sanitised, because "
     "a file may legitimately be named Ignore all previous instructions.pdf."),

    ("p",
     "The same tagging drives the browser and email subsystems: a web page is "
     "tagged untrusted on the way in, so a page instructing the agent to "
     "exfiltrate a contact list produces a request the broker will not approve. "
     "An inbound email carrying override directives is quarantined at ingestion "
     "and its tool access revoked. Rule R-5 forbids auto-send outright."),

    ("h2", "C. Byte-Budget Resource Accounting"),

    ("p",
     "The classic decompression attacks declare their size in metadata, and the "
     "size in the metadata is the attack. A zip entry claiming 600 MB inside a "
     "600 KB archive is a request to materialise 600 MB, and a limit that reads "
     "the declared value has already agreed to it. We therefore enforce every "
     "ingestion budget against bytes actually read through the decompressor (3):"),

    ("eq", {
        "num": 3,
        "text": "bytes_read( r )  ≤  B( r )    ∀ r ∈ { compressed, expanded, entity, element "
                "},",
        "note": "checked at the point of extraction, never at the point of "
                "declaration.",
    }),

    ("p",
     "Constraint (3) covers the compressed entry, the decompressed output, the "
     "nesting depth of XML entities and the number of elements. A 600 MB zip bomb "
     "is refused in 311 ms with a resident-set increase of 3 MB — the cost of the "
     "refusal, not of the payload. A zip-slip entry naming ../../../../etc/passwd "
     "is rejected before the path is opened. Seven levels of nested entity "
     "expansion complete instantly; an external entity pointing at "
     "file:///etc/hostname is not resolved, and a test asserts its contents never "
     "appear in any output. Across 200 random byte strings per parser and three "
     "parsers, no input produced a panic."),

    ("p",
     "Document identity is a SHA-256 digest [30] of the extracted bytes, "
     "computed incrementally so that a streamed file is identified without "
     "buffering it whole."),

    ("h2", "D. Compile-Time Panic-Strategy Assertion"),

    ("p",
     "Our parser guard wraps every extraction in a catch_unwind so a panic "
     "inside a third-party PDF or XML library becomes a clean error rather than "
     "process death. Eight tests covered it, all of them passed, and none of "
     "them tested anything. The release profile set panic = “abort”, under "
     "which catch_unwind does not catch: the process simply dies. Opening a "
     "malformed PDF terminated the application instantly, losing an unsaved "
     "conversation, while the test suite reported the path as handled."),

    ("p",
     "No unit test could have caught this, and the reason is instructive. cargo "
     "test always builds with unwinding regardless of the profile, and so do "
     "build scripts. We wrote both a unit test and a build-script check; both "
     "appeared to work, and both were discarded once we measured them properly. "
     "Running the full suite under the aborting profile produced 215 passes, "
     "identical to the unwinding run. The build script read CARGO_CFG_PANIC and "
     "reported unwind. An equivalent release binary terminated with SIGABRT and "
     "exit status 134. The fix is a compile-time assertion inside the crate:"),

    ("mono",
     "  #[cfg(panic = \"abort\")]\n"
     "  compile_error!(\"the extraction guard relies on unwinding; \\\n"
     "                    set panic = \\\"unwind\\\" in the release profile\");"),

    ("p",
     "The guard is now checked against the build configuration rather than the "
     "runtime, so a profile that would silently disable it fails the build with a "
     "message naming the fix. The same function had a second defect: it swapped "
     "the process-global panic hook on every call, so two documents ingested "
     "concurrently could race and leave one thread's silencing hook installed "
     "permanently. The hook is now installed once, and a thread-local depth "
     "counter decides whether to silence."),

    ("h2", "E. Heading-Trail Chunking with Rank Fusion"),

    ("p",
     "Structure-aware chunking was our first hypothesis about retrieval quality "
     "and it was half wrong. The hypothesis was right that the heading under "
     "which a passage appears carries information the passage does not repeat; "
     "the mechanism was wrong, because we did not model the heading as a separate "
     "field and filter on it. We prefixed each chunk with its heading path — “3.2 "
     "Termination > Notice period > Obligations” — and let the lexical retriever "
     "see it as ordinary text. The effect is concentrated where predicted: "
     "overall recall@4 rises from 0.615 to 0.769, and on heading-scoped probes, "
     "where the answer lives under a heading whose words never appear in the "
     "passage body, recall rises from 0.167 to 0.833."),

    ("p",
     "Fusion across the lexical and dense retrievers uses Reciprocal Rank Fusion "
     "[15]. We chose it over a weighted score combination for the reason its "
     "authors give: BM25 scores are unbounded and cosine similarities live in "
     "[−1, 1], so any direct mixture is miscalibrated by construction. Rank "
     "fusion is scale-free (4):"),

    ("eq", {
        "num": 4,
        "text": "RRF( d )  =  Σ_{ i ∈ R }  1 / ( k + rank_i( d ) ),      k = 60,",
        "note": "R is the set of retriever rankings, rank_i(d) the rank of "
                "document d in ranking i.",
    }),

    ("p",
     "Retrieval quality is reported with the usual two metrics of (5) over the "
     "set Q of answerable questions, with hits(q) the set of documents that "
     "answer q and Top_k(q) the top-k retrieved set:"),

    ("eq", {
        "num": 5,
        "text": "recall@k = |{ q : hits(q) ∩ Top_k(q) ≠ ∅ }| / |Q|,        MRR = (1/|Q|) Σ_q "
                "1 / rank_q.",
    }),

    ("p",
     "Our one hyperparameter is the conventional k = 60, and we did not tune it."),

    ("h1", "VI. EVALUATION"),

    ("h2", "A. Research Questions"),

    ("bullets", [
        "RQ1 — Does hybrid retrieval beat either component alone, and does "
        "attaching heading trails to chunks change answer quality?",

        "RQ2 — Does a provenance-tagged capability boundary refuse injected "
        "instructions that reach the model, and does it do so without a "
        "detector?",

        "RQ3 — Do byte budgets actually bound the cost of hostile containers, "
        "rather than merely describing it?",

        "RQ4 — What does the test suite catch, and what does it fail to catch?",
    ]),

    ("h2", "B. Testbed"),

    ("p",
     "Table II lists the configuration. All measurements come from one machine, "
     "and it is not the deployment target: the build and test host has 1.9 GiB of "
     "RAM against the 5.7 GiB usable budget the product is designed for, so the "
     "numbers below are a lower bound. The retrieval corpus is 22 chunks chosen "
     "to be adversarial — two conflicting human-resources handbooks where one "
     "supersedes the other, a contract, an invoice and a runbook — and the "
     "question set is 30 probes over it, four unanswerable by construction and "
     "excluded from retrieval averages. Refusing to answer is a generation-stage "
     "property, scored separately: a similarity search over a small corpus always "
     "returns a nearest neighbour, and “nearest” is not “relevant”. Retrieval is "
     "evaluated at k = 4."),

    ("caption", "TABLE II. Testbed and experimental configuration."),
    ("table", {
        "cols": ["Parameter", "Value", "Parameter", "Value"],
        "rows": [
            ["Host OS", "Linux 6.1.158, x86-64", "Inference", "llama.cpp b4585 sidecar, loopback"],
            ["CPU", "Intel Xeon @ 2.60 GHz, 2 cores", "Chat model", "≤3 B, Q4_K_M, one resident"],
            ["Host RAM", "1.9 GiB (target: 8 GB, 5.7 usable)", "Dense side", "384-dim hashed stand-in"],
            ["Toolchain", "rustc 1.99.1, Node 20.20.2, Python 3.13.16", "Retrieval", "FTS5 BM25 + dense, RRF k=60"],
            ["Native closure", "739 Debian 13 packages, 10 pkg-config modules", "Corpus", "22 chunks, 30 questions (26 scored)"],
            ["Test suite", "515 Rust assertions (493 fns), 54 JS assertions", "k", "4"],
        ],
        "widths": [0.16, 0.34, 0.16, 0.34],
        "rule": 2,
    }),

    ("h2", "C. Retrieval Results (RQ1)"),

    ("fig", {
        "file": "fig2_retrieval.png",
        "height_in": 1.72,
        "width_in": 7.10,
        "caption": "Fig. 2. recall@4 by configuration, with Wilson 95% intervals over the 26 "
                   "answerable questions. The intervals overlap, so the configurations are not "
                   "separated by this corpus.",
    }),

    ("caption", "TABLE III. Retrieval quality at k = 4, with Wilson 95% intervals."),
    ("table", {
        "cols": ["Configuration", "recall@4", "95% CI", "MRR", "prec@4"],
        "rows": [
            ["Hybrid (BM25 + dense, RRF)", "0.769", "[0.579, 0.890]", "0.667", "0.202"],
            ["BM25 only", "0.731", "[0.539, 0.863]", "0.635", "0.301"],
            ["Dense only", "0.692", "[0.500, 0.835]", "0.571", "0.183"],
            ["Hybrid, heading trail removed", "0.615", "[0.425, 0.776]", "0.494", "0.154"],
        ],
        "widths": [0.36, 0.15, 0.20, 0.14, 0.15],
        "rule": 2,
    }),

    ("p",
     "Hybrid retrieval beats both components on recall and on MRR, which is the "
     "argument for fusing them: the two retrievers fail on different questions. "
     "The margin over BM25 alone is 0.038 recall, and Figure 2 shows why we claim "
     "no more: the intervals overlap at n = 26, and the comparison that matters "
     "is paired, which requires per-question outcomes we did not retain. "
     "Precision at four is lowest for hybrid, as expected: a fused ranker pulls "
     "in documents one retriever liked and the other did not, raising recall and "
     "costing precision. The ablation is the result we do defend: removing the "
     "heading trail costs 0.154 recall overall and 0.666 on heading-scoped "
     "probes, a property of the indexed text rather than of a ranking artefact."),

    ("caption", "TABLE IV. Recall@4 by probe category (hybrid configuration)."),
    ("table", {
        "cols": ["Probe category", "Hybrid", "No heading trail", "What it tests"],
        "rows": [
            ["Disambiguation", "1.000", "—", "A superseded 2019 handbook must not outrank the current one"],
            ["Exact", "0.857", "—", "Invoice numbers, account codes, purchase-order references"],
            ["Heading-scoped", "0.833", "0.167", "The answer sits under a heading absent from the passage body"],
            ["Paraphrase", "0.500", "—", "Query wording shares no vocabulary with the document"],
        ],
        "widths": [0.16, 0.10, 0.15, 0.59],
        "rule": 2,
    }),

    ("p",
     "Table IV breaks the result down by probe category. Two failures constrain "
     "what the system can do. A query for a specific bank account number is found "
     "by BM25 alone and lost by hybrid at k = 4, because noise from the stand-in "
     "embedder crowds the exact match out of the fused top four; if that survives "
     "a real encoder, the repair is a score floor on the dense side, not a "
     "re-weighting of the fusion. A second query about terminating a services "
     "agreement fails in every configuration, because the document says “bring "
     "this arrangement to an end” and no lexical or hashed-dense signal connects "
     "that to the question. Neither is a retrieval-configuration problem; both "
     "are consequences of the encoder standing in."),

    ("h2", "D. Adversarial Ingestion (RQ2, RQ3)"),

    ("caption", "TABLE V. Hostile inputs against the ingestion pipeline."),
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

    ("p",
     "The provenance boundary is exercised by the fifth row of Table V and by "
     "the email and browser suites: an injected directive inside a real DOCX "
     "produces a call whose provenance is document text, and the broker refuses "
     "it without inspecting the text. The remaining rows answer RQ3 directly: "
     "the byte budget is not a description of what a well-behaved archive "
     "consumes, it is what bounds the cost of a hostile one. The 3 MB "
     "resident-set increase for a 600 MB declared payload is the entire cost of "
     "the refusal."),

    ("h2", "E. What the Tests Caught (RQ4)"),

    ("caption", "TABLE VI. Defect classes caught by the project's own test suite."),
    ("table", {
        "cols": ["Class", "Representative defect", "Consequence had it shipped"],
        "rows": [
            ["Panic containment", "catch_unwind under panic = “abort”", "Application death on malformed PDF; unsaved chat lost"],
            ["Buffer state", "SHA-256 update discarded buffered bytes on one split path", "Corrupted document identity on 1 of 200 splits"],
            ["Citation integrity", "Chunk spanning a page break inherited the earlier page", "Citation pointed at a page not containing the quote"],
            ["Unicode boundaries", "Lowercasing U+0130 changes byte length; split offsets advanced by 1", "Panic or truncation on adversarial input"],
            ["Intent matching", "Substring match hijacked unrelated turns", "“open my profile picture” launched the file manager"],
            ["Concurrency", "Process-global panic hook swapped per call", "Silent hook leak across concurrent ingests"],
            ["Dependency drift", "Debian index named a superseded point release", "Build aborted with an empty sysroot and no error"],
        ],
        "widths": [0.17, 0.38, 0.45],
        "rule": 2,
    }),

    ("p",
     "Table VI lists the defect classes the suite caught. The Unicode class "
     "deserves a note, because it is a family of bugs ordinary testing does not "
     "find. Lowercasing U+0130, a single character, yields a three-byte sequence "
     "from a two-byte input, so code that computes a byte offset before "
     "lowercasing and slices afterwards can cut a character in half. We removed "
     "it by adopting ASCII-only case folding for attacker-controlled text, which "
     "preserves byte lengths, and by advancing split offsets with char_indices "
     "and len_utf8 rather than byte arithmetic. One earlier test passed for the "
     "wrong reason: a test asserting that PDF and DOCX parsing returned errors "
     "kept passing after the formats were implemented, because the fixtures were "
     "malformed."),

    ("h2", "F. Observation-Space Cost"),

    ("p",
     "Fig. 3 shows the context an observation costs: 2-5 KB for an "
     "accessibility tree against 1.4 MB for a screenshot."),

    ("fig", {
        "file": "fig3_context.png",
        "height_in": 1.02,
        "width_in": 7.10,
        "caption": "Fig. 3. Context carried per observation. Accessibility-tree observation "
                   "rather than pixels is what keeps the agent inside the 8 GB budget; "
                   "ComponentBench [21] shows the same choice moving task success by 34.2 "
                   "points.",
    }),

    ("h1", "VII. LIMITATIONS AND THREATS TO VALIDITY"),

    ("bullets", [
        "The dense retriever is a hashed bag-of-words stand-in, not a trained "
        "encoder. Every number in the dense column, and the paraphrase category "
        "in particular, is a floor rather than a measurement of the product, and "
        "no number from that column should be quoted outside this paper.",

        "The evaluation corpus is 22 chunks and the question set is 30 probes. The "
        "Wilson intervals in Table III are wide and overlapping, so the retrieval "
        "comparison is directional rather than significant. Nothing here establishes "
        "behaviour at 10⁵ chunks, and our justification for a brute-force vector "
        "scan is arithmetic, not a benchmark.",

        "Nothing has run inside the packaged application: we compile and test "
        "the real Tauri crate, but no cargo tauri build has been executed here "
        "and no display server was available.",

        "PDF extraction was verified against files produced by one writer; "
        "multi-column layouts, ligatures, subset fonts and scanned pages are "
        "untested.",

        "Accessibility metadata coverage is an unmeasured precondition for our "
        "automation results. LUMOS [22] makes the same observation generally, "
        "and notes that coverage of that metadata determines whether the "
        "approach works at all.",

        "All measurements come from one machine class, and the host is weaker "
        "than the deployment target. We have not characterised the residency "
        "rule on systems with slow NVMe or a compressing memory manager, both "
        "of which change the cost of a swap without changing its correctness.",
    ]),

    ("h1", "VIII. CONCLUSION"),

    ("p",
     "Local language-model deployment is usually framed as a degraded cloud, "
     "and the frameworks built for it inherit cloud assumptions that do not "
     "survive contact with 8 GB of RAM and a user who will not wait. We built "
     "Orion against physical constraints instead, and the five contributions "
     "above are what survived that reframing: deciding model swaps on artefact "
     "identity rather than role identity; refusing privileged calls on "
     "provenance rather than detected intent; accounting for bytes read rather "
     "than bytes declared; asserting the panic strategy at compile time rather "
     "than trusting a test to observe it; and attaching heading context to "
     "chunks so structure becomes retrievable text. None of these is a large "
     "idea. Each is small, checkable, and each was arrived at by being wrong "
     "first, which is the honest shape of systems work at this scale."),

    ("h1s", "ACKNOWLEDGMENT"),

    ("p",
     "This work was carried out as independent research and development without "
     "external funding; the author declares no sponsorship or institutional "
     "support."),

    ("h1s", "CONFLICT OF INTEREST"),

    ("p",
     "The author declares no conflict of interest; no commercial product derived "
     "from the described system was on the market during the preparation of this "
     "manuscript."),

    ("h1s", "DATA AND CODE AVAILABILITY"),

    ("p",
     "The build scripts, the 30-question retrieval suite with its fixtures, the "
     "adversarial input generators and both test suites are included with the "
     "artifact. The native dependency closure is resolved from a Debian 13 pool "
     "by an idempotent, offline-capable script, and the paper is generated from a "
     "single content source with a builder that reports the page count it "
     "computed."),

    ("h1s", "ETHICS AND BROADER IMPACT"),

    ("p",
     "The system processes documents, mail and web pages that the user supplies, "
     "and the two-domain boundary keeps that material from becoming an "
     "instruction. Deployment on shared hardware is restricted by a salted PIN "
     "lock and an eight-minute inactivity timer that unloads the model. The work "
     "depends only on open-weight models under permissive licences, and it makes "
     "no claim of the kind a proprietary assistant would be tempted to make."),

    ("h1s", "AUTHOR BIOGRAPHY"),

    ("p",
     "Kuldeep Yadav is an independent developer and researcher working on "
     "local-first AI systems. He designed and built Orion end to end, from the "
     "Rust retrieval engine to the Tauri desktop shell, and maintains the "
     "benchmark suite and reproducible build reported here."),

    ("h1", "REFERENCES"),

    ("refs", None),
]

# --------------------------------------------------------------------------
# References - exactly 30
# --------------------------------------------------------------------------

REFERENCES = [
    "J. Frantar, S. Ashkboos, S. Hooper, et al., “GPTQ: Accurate Post-Training "
    "Quantization for Generative Pre-trained Transformers,” in Int. Conf. on "
    "Learning Representations (ICLR), 2023.",
    "T. Chen, S. Li, H. Zhang, et al., “AWQ: Activation-aware Weight "
    "Quantization for LLM Compression and Acceleration,” in Proc. Conf. on "
    "Machine Learning and Systems (MLSys), 2024.",
    "T. Dettmers, N. Shazeer, M. Andreev, et al., “LLM.int8(): 8-bit Matrix "
    "Multiplication for LLMs at Scale,” arXiv:2208.07339, 2022.",
    "T. Dettmers, A. Pagnoni, S. Holtzman, and Y. Yang, “QLoRA: Efficient "
    "Finetuning of Quantized LLMs,” arXiv:2305.14314, 2023.",
    "Meta AI, “The Llama 3 Herd of Models,” arXiv:2407.21783, 2024.",
    "Y. Ji, W. Chen, X. Xie, et al., “FlexInfer: Breaking Memory Constraint "
    "via Flexible and Efficient Offloading for On-Device LLM Inference,” "
    "arXiv:2503.03777, 2025.",
    "Z. Shen, Y. He, Z. Wang, Y. Zhang, G. Sun, W. Ye, and A. Li, “EdgeLoRA: "
    "An Efficient Multi-Tenant LLM Serving System on Edge Devices,” "
    "arXiv:2507.01438, 2025.",
    "S. Robertson and E. Zaragoza, “The Probabilistic Relevance Framework: "
    "BM25 and Beyond,” Foundations and Trends in Information Retrieval, vol. "
    "3, no. 4, 2009, pp. 333–389.",
    "N. Reimers and I. Gurevych, “Sentence-BERT: Sentence Embeddings using "
    "Siamese BERT-Networks,” in Proc. Conf. Empirical Methods in Natural "
    "Language Processing (EMNLP-IJCNLP), 2019, pp. 3982–3992.",
    "S. Xiao, Z. Liu, P. Zhang, N. Muennighoff, et al., “C-Pack: Packaged "
    "Resources To Advance General Chinese Embedding,” arXiv:2309.07597, 2023.",
    "V. Santhanam, P. Selvaraju, A. Yellapragada, et al., “DistilBERT — A "
    "Distilled Version of BERT,” arXiv:2010.01184, 2020.",
    "V. Karpukhin, O. Khattab, D. Piao, et al., “Dense Passage Retrieval for "
    "Open-Domain Question Answering,” in Proc. Conf. Empirical Methods in "
    "Natural Language Processing (EMNLP), 2020, pp. 6769–6781.",
    "T. Malkov and D. Yashunin, “Efficient and Robust Approximate Nearest "
    "Neighbor Search Using Hierarchical Navigable Small World Graphs,” IEEE "
    "Trans. Pattern Anal. Mach. Intell., vol. 42, no. 4, 2020, pp. 824–836.",
    "P. Lewis et al., “Retrieval-Augmented Generation for Knowledge-Intensive "
    "NLP Tasks,” in Advances in Neural Information Processing Systems "
    "(NeurIPS), 2020, pp. 9459–9474.",
    "G. V. Cormack, C. L. A. Clarke, and S. Büttcher, “Reciprocal Rank Fusion "
    "outperforms Condorcet and individual Rank Learning Methods,” in Proc. "
    "32nd ACM Int. Conf. Research and Development in Information Retrieval "
    "(SIGIR), 2009, pp. 758–759.",
    "K. Greshake, S. Abdelnabi, S. Mishra, C. Endres, T. Holz, and M. Fritz, "
    "“Not What You’ve Signed Up For: Compromising Real-World LLM-Integrated "
    "Applications with Indirect Prompt Injection,” in Proc. 16th ACM Workshop "
    "on Artificial Intelligence and Security (AISec), 2023, pp. 79–90.",
    "J. Yi, Y. Xie, B. Zhu, E. Kiciman, G. Sun, X. Xie, and F. Wu, "
    "“Benchmarking and Defending Against Indirect Prompt Injection Attacks on "
    "Large Language Models,” arXiv:2312.14197, 2023.",
    "E. Debenedetti, J. Zhang, M. Balunović, L. Beurer-Kellner, M. Fischer, "
    "and F. Tramèr, “AgentDojo: A Dynamic Environment to Evaluate Prompt "
    "Injection Attacks and Defenses for LLM Agents,” in Advances in Neural "
    "Information Processing Systems (NeurIPS) Datasets and Benchmarks Track, "
    "2024.",
    "OWASP Foundation, “OWASP Top 10 for Large Language Model Applications,” "
    "2025.",
    "National Institute of Standards and Technology, “Artificial Intelligence "
    "Risk Management Framework (AI RMF 1.0),” NIST AI 100-1, 2023.",
    "“ComponentBench: Diagnosing Component-Level Failures in Computer-Use "
    "Agents,” arXiv:2608.18307, 2026.",
    "“LUMOS: A Semantic Operating-System Layer for Accessibility-Grounded AI "
    "Agents,” arXiv:2606.30697, 2026.",
    "Tauri Team, “Tauri: Build smaller, faster, and more secure desktop "
    "applications with a web frontend,” software documentation, 2024–2026. "
    "[Online]. Available: https://tauri.app",
    "ggml-org, “llama.cpp: LLM inference in C/C++,” software repository, "
    "2023–2026. [Online]. Available: https://github.com/ggml-org/llama.cpp",
    "SQLite Project, “SQLite Full-Text Search Version 5 (FTS5),” software "
    "documentation, 2024. [Online]. Available: https://sqlite.org/fts5.html",
    "asg017, “sqlite-vec: Vector search for SQLite,” software repository, "
    "2024–2026. [Online]. Available: https://github.com/asg017/sqlite-vec",
    "Microsoft, “Windows AppContainer,” platform documentation, 2024. "
    "[Online]. Available: "
    "https://learn.microsoft.com/windows/win32/secauthz/appcontainer-isolation",
    "P. J. Denning, “The Working Set Model for Program Behavior,” Commun. ACM, "
    "vol. 11, no. 3, 1968, pp. 152–166.",
    "P. J. Denning, “Virtual Memory,” ACM Comput. Surv., vol. 2, no. 3, 1970, "
    "pp. 149–160.",
    "National Institute of Standards and Technology, “Secure Hash Standard "
    "(SHS),” FIPS PUB 180-4, 2015.",
]

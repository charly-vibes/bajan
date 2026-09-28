# Thematic Report — Building Knowledge Graphs from LLM/Agent Content

**Generated:** 2026-09-28

**TL;DR:** The corpus shows a full spectrum of KG construction — from deterministic,
structure-derived document graphs (no extraction LLM at all) to fully LLM-extracted
entity/relation graphs with provenance baked in. The consensus that emerges: **schema-first beats
schema-less**, **entity resolution is the underrated hard part**, **provenance must be
engineered into the data structure (a graph), not logged afterwards**, and **most teams
over-build** — several credible speakers argue you shouldn't reach for a graph DB (or a
KG at all) until multi-hop traversal is a proven need, measured by evals.

**Purpose:** a self-contained synthesis of how the corpus builds knowledge graphs from
unstructured content (talks, docs, agent activity), what works, what fails, and where the
practice disagreements are. Passes the amnesia test: readable with zero prior context.

**Sources:** all from the corpus in this repo; full transcripts in `transcripts*/txt/`,
metadata in `analysis/data/talks.json`. Method note: claims are speaker attributions from
stage, mostly transcribed from auto-captions; single-speaker accounts, not verified facts.
Vendor stakes are flagged where they exist (Neo4j and Zep both sell graph infrastructure).

---

## Glossary

| Term | Meaning as used in the corpus |
|---|---|
| **Triple** | subject–predicate–object; the atomic unit of a knowledge graph |
| **Episode** | (Graphiti/Zep) a verbatim source artifact stored as a graph node — the provenance anchor |
| **Entity resolution** | deciding that "J. Smith" and "John Smith" are one node; the merge step |
| **Provenance / lineage** | traceable links from every derived fact back to its source(s) |
| **Business ontology** | domain concepts named the way humans talk (customer, account — not `f_name`) |
| **Technical ontology** | metadata of every data source: where it sits, its schema, mappings |
| **Execution traces** | recorded agent runs: what was tried, whether it worked — a self-learning signal |
| **Decision trace** | (Context Graphs) the captured why behind one operational decision |
| **Community detection** | clustering densely interlinked nodes (Leiden/Louvain) to surface themes |
| **GraphRAG** | Microsoft-paper retrieval pattern: LLM-extracted entity graph + community summaries |
| **Schema-first extraction** | the LLM extractor is given a target node/edge schema to fill, rather than free-form triples |
| **Event-sourced graph** | (Nakajima/ActiveGraph) an immutable event log is the ground truth; the graph is a projection of it, queried at read time |

---

## 1. The two anchor talks

### 1.1 [Citation Needed: Provenance for LLM-Built Knowledge Graphs — Daniel Chalef, Zep AI](https://youtu.be/H7puB0RwJMM) (`transcripts_aieng/txt/206 - …[H7puB0RwJMM].txt`)

The problem: LLM synthesis is non-deterministic and **destroys the paper trail**. His failure
mode: an agent hands a doctor "patient has penicillin allergy," synthesized from three sources
(EHR record, PDF lab report, patient-typed intake chatbot). Without lineage you can't know the
critical fact came from the *patient*, not a verified clinical source.

**Why a source ID on the fact doesn't work** (the naive fix he explicitly rebuts): in
deterministic pipelines one value has one source; in LLM pipelines (a) facts are synthesized
from *multiple* sources, (b) entities get merged ("J. Smith" + "John Smith"), so John's facts
come from many places, (c) new data invalidates old facts, so "the store keeps changing
underneath your pointer." Lineage must be **an evolving set that survives mutation**.

**How they do it (Graphiti, the OSS framework behind Zep)** — pipeline details below are
from the talk and its Q&A; wording normalized from captions:**

- Sources are stored **verbatim as nodes** ("episodes"); every derived artifact is also a
  node; a fact is a hydrated triple (two entity nodes + an edge). Tracing a fact to its
  origin is **just a graph walk**.
- **Lineage survives mutation:** when two entities merge, the merged entity keeps all source
  links from both (otherwise lineage is silently dropped). When a fact is invalidated, an
  `invalid` date goes on the mutated edge and the source episodes that caused the mutation
  are recorded against the fact.
- **Metadata projection:** tag episodes once at ingestion (e.g., `EHR`); all entities and
  facts derived from them inherit the tag. Filtering "facts from verified clinical sources"
  is then a tag check during the walk — one tagging action supports veracity evaluation.
- **Mixed-trust parents are a policy problem, not a graph problem.** Two facts, both with
  three parents, one unverified: for an allergy flag, *any* verified parent suffices (missing
  it is deadly); for consent-on-file, *every* parent must be verified. The graph exposes
  which parents carry the tag; **your agent applies the business rule** — deliberately not
  baked into the store.
- **Deletion/GDPR:** a fact is deleted only if **no remaining episode supports it**. Delete
  the intake-chat source and the allergy fact survives (two parents remain) while the
  contact-preference fact dies (single parent). The rule is trivial *because the links exist*.
- **Ingestion pipeline:** a **single-shot LLM extraction** of entities + relationships +
  candidate facts (subject-verb-object), then a **dedup + deconfliction** pass that mutates
  existing facts (e.g., "Daniel loves Adidas" → invalidated when "Daniel returned the shoes").
  Explicit cost discipline: **avoid LLMs wherever traditional IR/NLP works** — simhash,
  entropy-based dedup — because it's "far cheaper, far faster, far more deterministic."
- **What they admit:** provenance and graph construction are **expensive**; they've invested
  heavily in cutting cost/latency. Also: edge-weight/relevancy provenance lives in a
  **separate data structure** in Zep, *not* in the graph itself — even the vendor doesn't put
  everything in the graph.
- **On markdown/file-based memory:** "markdown suffers from provenance" — mutating lines in a
  file destroys lineage; files break down in multi-agent, multi-user, multi-source enterprise
  scenarios (concedes they work fine for desktop, single-user cases).

### 1.2 [Thinner Agents on a Smarter Substrate — Emil Eifrem, Neo4j](https://youtu.be/VGN22pPpb-8) (`transcripts_aieng/txt/215 - …[VGN22pPpb-8].txt`)

Not a document-extraction KG but an **organizational** one. The anti-pattern he attacks: every
agent team re-wires its own data access from scratch — finds sources manually across a hundred
databases + Snowflake + S3, guesses trust and permissions, hardcodes it all in code and
prompts. Violates DRY; nothing learns; a source move forces manual rewiring of every agent.

**How they do it — three pillars of an ontology-based semantic layer:**

1. **Business-facing ontology:** the key concepts (customers, accounts, checks) and how they
   relate, named the way humans in the org actually talk. Explicitly simple — he pushes back
   on people making ontologies complex ("you don't say `if_underline_name`, you have a
   customer with a first name").
2. **Technical ontology:** all data-source metadata — where the 14 Oracle DBs, Snowflake,
   Databricks, S3 buckets sit, their schemas — plus a **mapping** between the two layers
   (`customer.first_name` ↔ Oracle column `f_name`).
3. **Execution traces:** agents walking the graph leave runtime signals — what was tried,
   what worked, a success score. Next invocation, an agent that succeeded with the DMV lookup
   is more likely to choose it again. **Trust is both top-down (human-curated) and bottom-up
   (empirical traces).**

Result claims: data discovery, trust, DRY, and cross-agent learning all become properties of
the substrate; agents get **thin**. Also encodes *business processes* in the ontology so
process-guided agents follow them. Note: "markdown files and skills are part of the solution,
but they are not the solution" — his quote of himself via swyx: "you cannot vibe code with
just markdown files."

---

## 2. The construction spectrum: how the corpus actually builds KGs

A taxonomy of four families, roughly ordered by how much LLM work the ingestion path does:

**a) Deterministic / structural graphs — no extraction LLM.**
[AI on Your Lakehouse — Zach Blumenfeld, Neo4j](https://youtu.be/kRkcNOsRyYg) (`204`): build
the graph from what the documents already have — folders → documents → sections → links
(a "lexical or document-structure graph"). Benefits he lists: **idempotent** loads, faster,
no LLM in the loop. Then add intelligence *cheaply*: **Leiden community detection** over the
structure alone produces meaningful thematic clusters (brakes, BCM…) with no tagging —
his "lightweight GraphRAG," explicitly contrasted with Microsoft-style heavy entity
extraction. Also: a **metadata graph** (Neocarta) built by sucking table/column/join-path
metadata out of BigQuery via MCP — schema-as-graph so an agent can traverse joins instead of
guessing them.

**b) Schema-first LLM extraction.**
[A Practitioner's Guide to Graphs — Tim Ainge, Good Collective](https://youtu.be/3ySF0I5iE_0)
(`248`) makes this the central lesson. Naive "extract triples, agent figures it out" on a
pancake recipe yields a messy, useless graph. Fixes, in escalation order:
1. **Give the extractor a schema to fill** (a Recipe has Ingredients with quantities, ordered
   Steps) → "instantly way more meaningful," consistent node/edge types make relationships
   queryable.
2. **Ontology instructions in the prompt matter as much as the schema** — "lowercase
   ingredient names, metric units" — normalization at extraction time.
3. **Don't trust the prompt:** normalize deterministically anyway ("the best prompt in the
   world isn't bulletproof").
4. **Entity matching with embeddings**, not naive string maps: merges garlic/minced-garlic,
   cumin/cumin-seeds without knowing the vocabulary in advance. "Graph techniques and AI
   techniques in hybrid give the best result."

**c) Full LLM entity/relation extraction with conflict handling.**
Graphiti (§1.1), Novo Nordisk at scale (NER + entity resolution over 60M documents / billions
of nodes — Eifrem on [Latent Space](https://youtu.be/yyuVR-ML9X8), who calls entity
resolution "such an under-discussed area in AI engineering").

**d) Agentic graph building.**
[CrabRAG — Stephen Chin, Neo4j](https://youtu.be/Q0VkgCyNVUg) (`214`): "if you're not a graph
expert, guess what, Claude is" — have the coding agent write the Cypher, build entity
extractors, and write each action into the graph as it works. Human role shifts to
inspecting the graph and fixing extraction/duplicate nodes.

**e) Event-sourced graphs — log-centric, graph as projection.**
[Active Graph Agent Runtime (BabyAGI 4) — Yohei Nakajima, Untapped Capital](https://youtu.be/khVX_BUnEwU)
(`213`): the inversion of (a)–(c) — don't build the graph, build **around the log**: an
immutable, typed event log is the ground truth of the agent, the graph is a projection of
it, and "behaviors" react to graph changes and emit new events. You get graph queryability
with full replay/audit — but "you have to log everything correctly." This is the corpus's
sharpest unresolved disagreement with §4.2 (Chalef: append-only logs are unmanageable at
scale; Nakajima: the log *is* the provenance substrate).

---

## 3. What works (with claimed evidence)

| Practice | Evidence from corpus |
|---|---|
| **Vector seeds + graph traversal (hybrid retrieval)** | Eifrem's canonical pattern: vector + BM25 find ~100 candidate docs, **then traverse** for full context (author rank, PageRank). "Not graph or vector — vector search in combination with traversing." CrabRAG's A/B demo: same data, vector store gives fuzzy near-misses, graph store (vector-seeded one-hop traversal) finds the actual open port. |
| **Provenance as graph structure** | Chalef: compliance (GDPR erasure = reverse graph walk), veracity filtering by source tags, debugging "why do I have this fact." |
| **Schema-first extraction** | Ainge's escalation from triple-soup to schema'd graph; consistent types are what make relationships interrogable. |
| **Graph-native algorithms on cheap graphs** | Ainge: personalized PageRank finds the landmark case *not directly cited* (Miranda via citation chain); shortest path in a code graph explains why a change broke checkout — **40% reduction in tool calls** in their .NET evaluation; subgraph matching finds code *shapes* (decorator pattern) without knowing symbols — "not easy with other tools." Blumenfeld: Leiden communities give global "what themes exist" answers with zero LLM extraction. |
| **Execution traces as a learning signal** | Eifrem (215): bottom-up trust + cross-agent learning; also the observed industry flip — agents now **lead with generic text-to-Cypher and extract specialized tool functions from failures**, instead of the reverse. (He still fine-tunes their internal text-to-Cypher model + regex post-processing: "models out of the box are not good enough for all situations.") |
| **Extraction-time normalization + cheap dedup** | Ainge (instructions + deterministic fallback); Chalef (simhash/entropy-based dedup over LLM passes — cheaper, faster, more deterministic). |
| **Idempotent, deterministic loading** | Blumenfeld: re-runnable loads, no LLM dependency at ingestion, hierarchy carried in URIs. |

Production-scale existence proofs in the corpus: Novo Nordisk (60M docs, billions of nodes,
life-science researcher tooling), a mortgage lender whose KG-driven best-path system
(+20% conversion) is now fully customer-facing and automated, 30% of Neo4j's 2026 AI
conversations with global banks (all Eifrem, all vendor-sourced).

---

## 4. What doesn't work / failure modes the corpus names

1. **Schema-less extraction ("just pull triples").** Ainge's opening demo: a vague,
   unusable graph. Doubly wasteful because downstream algorithms (PPR, community detection)
   need meaningful edge types to shine.
2. **Source-ID-on-the-fact lineage.** Chalef's core rebuttal — breaks under multi-source
   synthesis, entity merges, and mutation. Also **append-only logs** get "very hard to manage
   at scale" when the store keeps changing — though Nakajima (§2e) makes the immutable log
   the *ground truth* the graph projects from, so log-vs-graph as the lineage substrate is
   the corpus's sharpest open disagreement, not settled failure-mode knowledge.
3. **Markdown/file-based memory as a KG substitute at enterprise scale.** Chalef: provenance
   and mutation tracking break; multi-agent/multi-user breaks. Eifrem: "part of the solution,
   not the solution." (Counterpoint in §6.)
4. **Triplets as the memory unit** — [Supermemory's Shah](https://youtu.be/Io0mAsHkiRY)
   claims triplets "lead to worse performance because you have to traverse them a lot to get
   to any information" (to learn what a person likes you must find the person node, then
   walk 1–2 hops). Note
   the podcast host's retort: "this is the learning of every graph guy ever." Built custom
   extraction + non-triple graph instead.
5. **GraphRAG as an out-of-the-box win.** Joe Christian
   ([Graph Databases: When to Use Them (And When to Run Away)](https://youtu.be/7kXY-2fYdHI)):
   the Microsoft recipe feeds the **entire corpus through an LLM** — "quite expensive"; the
   hard part is the KG construction itself, not the traversal ("certainly easier to run some
   nice demo"). Yahoo once maintained a whole KG team; maintenance is the cost people forget.
6. **Reaching for a specialized graph DB by default.** Christian again: a KG is just triples —
   representable in a text file, CSV, relational DB (early Facebook ran on MySQL), or a small
   Python script. Questions to answer first: Do you need fast multi-hop traversal? How large
   is the graph? Does accuracy actually improve (measure **before** adding the DB)? Is another
   specialized DB in the serving stack worth the complexity? "When you ask people what
   GraphRAG is… 'you know what you need? A graph database.'" (The host's framing of his
   talk: 90% of production graph DBs the host has seen are "abused or introduce unnecessary
   complexity" — spoken in the intro, not by Christian on stage.)
7. **Big-bang enterprise KG projects.** Tanmai Gopal
   ([PromptQL](https://youtu.be/0uC6u0lJJl4), `027`): "we're not building gigantic knowledge
   graph knowledge bases for the company… that hasn't worked, won't work." A 100-year-old
   company's brain can't be built top-down in a two-year project — it has to be **grown**,
   piece by piece, by the people doing the work, with human-approved additions.
8. **Ontology complexity theater.** Eifrem: "a lot of people want to make ontologies really
   complex. But the core concepts are super simple." Coyle
   ([Why Agentic Systems Need Ontologies](https://youtu.be/Sir59K8ZDPU), `209`) grounds it:
   reuse existing taxonomies (schema.org, FOAF, Dublin Core, DBpedia) instead of reinventing;
   keep OWL/RDFS *off to the side* as constraint/inference layers (domain/range, transitive,
   functional properties catching "second refund on same order," "payout to support desk
   instead of buyer") rather than modeling everything.
9. **Trusting LLM output structurally.** Coyle's discipline: **Pydantic at the door, ontology
   at the ledger** — type-check parameters, then validate tool results against the ontology
   before the loop continues; agents should have no side effects until validated. LLM
   hallucination is "a feature," but the ontology is what keeps it "on its guardrails."

---

## 5. Good vs. bad practices (condensed)

| ✅ Good practice | ❌ Bad practice |
|---|---|
| Schema given to the extractor; ontology instructions as important as the schema | "Agent, extract triples, you figure it out" |
| Hybrid retrieval: vector/BM25 seeds → graph traversal for context | Vector-only lookup (opaque 0.7-cosine neighbors) or graph-only |
| Store sources verbatim; link every derived artifact back (provenance in the structure) | Source IDs stamped on facts; append-only logs; post-hoc lineage reconstruction |
| Dedup/normalize with cheap deterministic methods (simhash, entropy, regex); LLM only where needed | LLM calls for every dedup/conflict decision |
| Entity resolution as a first-class investment (embeddings for matching) | Naive string maps, or ignoring it ("J. Smith" ≠ "John Smith" silently forks entities) |
| Deterministic, idempotent ingestion of structure that already exists (folders, sections, links, schemas) | LLM-extracting structure the documents already have |
| Leiden/community detection for global questions over cheap graphs | Heavy per-chunk entity extraction + LLM community summaries when structure suffices |
| Measure with evals *before* adopting the graph stack (Christian) | Adopting because "everything is a graph" / a paper claimed gains |
| Thin agents + shared ontology/semantic layer with execution traces (multi-agent, multi-source orgs) | Every agent hardcoding its own data wiring |
| Business rules over lineage applied by the agent/policy layer (allergy: any verified parent; consent: all parents) | Baking one trust policy into the store |
| Deletion = graph walk: keep facts with surviving parents | Deleting derived facts by guesswork (or not deleting on GDPR request at all) |
| Grow the knowledge base from actual work, human-approved additions, named ownership (Gopal) | Two-year big-bang enterprise KG program; auto-writing agent memory |
| Validate with types (Pydantic) + ontology constraints around the agent loop | Free-running loops validated only by vibes |

---

## 6. Skeptics and counter-evidence (for balance)

- **Christian** (`7kXY-2fYdHI`) is the corpus's main anti-hype voice: KG ≠ graph DB; eval
  before adoption; HNSW graphs *inside* vector indexes mean "vector databases already use
  graphs internally" — the technique/database mapping people reach for is often wrong.
- **Iusztin & Bouchard** ([Turn 10,994 Notes Into Memory](https://youtu.be/ZRM_TfEZcIo),
  `336`) run the opposite experiment to §1.1: for a *personal* research wiki they
  **deliberately dropped** vector DBs, KGs, semantic search — "forget the infrastructure you
  think you need… add a lot of complexity" — for plain files + `index.yaml` references, with
  the LLM deriving an Obsidian-linked concept/entity graph *as a view*, not as storage.
  Token-efficiency via hierarchy (source page summary → wiki derivatives → raw source), and
  the wiki evolves from questions, not just ingestion.
- **Eifrem himself** concedes on memory: "in theory graph databases are fantastic for memory.
  In practice it might be overkill. Most people's memory… probably fits in a single file" —
  while insisting *context graphs* (institutional decision traces) are the real opportunity.
- **Context Graphs authors** ([Foundation Capital](https://youtu.be/zP8P7hJXwE0)): explicitly
  "don't think of context graphs as a technology architecture — think of it as a framework";
  no ideal data structure yet ("maybe relational with a graph layer"); of thousands claiming
  it works, "I've had maybe 30" real deployments. The bootstrapping problem (how do you
  instrument an organization to *start* capturing decision traces) is the unsolved core.
- **Vendor-stakes caveat:** the strongest KG-construction advice (Neo4j ×3, Zep) comes from
  graph vendors. The corpus contains **no neutral head-to-head** of KG vs. flat-file vs.
  relational memory at the same workload — the gap `memory-systems-report.md` §Gaps flags
  for memory applies here too.
- Existing repo note: `memory-systems-report.md` §4 already covers the graph-memory-vs-markdown
  argument from the memory angle; this report covers the *construction* angle and is
  consistent with it.

---

## 7. Cross-cutting takeaways

1. **Provenance is the KG's killer feature, not traversal.** The compliance/audit/deletion
   story (Chalef) and the explainability story (Eifrem: vector space is opaque, the graph is
   inspectable) both reduce to: links you can walk and show. If you build an LLM-derived
   graph without source links, you built a cache, not a knowledge base.
2. **Push intelligence upstream, but only where the data lacks it.** Structure that already
   exists (headings, links, DB schemas) should be loaded deterministically; LLM extraction
   is reserved for facts that only exist in prose. Both extremes in the corpus (pure-LLM
   GraphRAG, pure-deterministic) underperform the hybrid.
3. **Entity resolution + mutation handling is the real cost center** — merging, invalidation,
   dedup — and the corpus's most credible practitioners spend their engineering budget there,
   mostly with *non*-LLM techniques.
4. **Match the architecture to scale.** Personal/single-agent: files and references win
   (Iusztin, Eifrem's memory caveat). Org-scale, many agents × many sources: shared
   ontology/semantic layer with execution traces (Eifrem 215). Between them, start with
   triples in Postgres/a file and earn the graph DB through evals (Christian).
5. **The field is mid-flip on interaction patterns:** leading with hand-written query tools →
   leading with generic text-to-Cypher and extracting tools from failures; graphs moving from
   "retrieve documents" to "carry provenance, trust, and decision traces."

## Gaps

- No neutral benchmark in the corpus of LLM-extraction quality (precision/recall of extracted
  triples) — every "it works" is anecdotal or vendor-presented.
- Cost numbers for graph construction ("expensive," "we reduced it") are asserted, never
  quantified — the one session where numbers existed (Chalef Q&A) deferred them off-stage.
- The Context Graphs framework has ~30 credible deployments per its own authors; treat all
  enterprise KG-at-scale claims as early.
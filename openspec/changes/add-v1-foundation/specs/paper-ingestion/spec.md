# paper-ingestion

## ADDED Requirements

### Requirement: Staged, resumable pipeline
Ingestion SHALL run as ordered background stages — (1) layout analysis, (2) object extraction, (3) tables/equations/citations parsing, (4) local embeddings, (5) optional LLM enrichment — each writing its artifact with a `pipeline_version` and per-object confidence. The app SHALL remain fully usable during ingestion, and the paper SHALL be readable (raw view) after stage 1.

#### Scenario: Reading before enrichment completes
- **WHEN** stage 1 completes on a newly imported paper
- **THEN** the user can open and read the paper while later stages continue in the background with per-stage progress visible

#### Scenario: Interrupted ingestion resumes
- **WHEN** the app is quit during stage 3 and relaunched
- **THEN** ingestion SHALL resume from the incomplete stage without redoing completed stages

### Requirement: Performance budget
A typical 10-page arXiv paper SHALL complete stages 1–4 in under 30 seconds on the reference machine, with progress shown; LLM enrichment SHALL be background and resumable with no completion budget but visible per-stage progress.

#### Scenario: Ten-page paper on reference hardware
- **WHEN** a 10-page text-based PDF is imported on the reference machine
- **THEN** stages 1–4 finish in < 30 s and the UI never drops below interactive

### Requirement: Extraction confidence and hostile-PDF degradation
Every extracted object SHALL carry a confidence score. When a stage fails or confidence is low (scanned pages, malformed PDFs, dense math), the pipeline SHALL degrade per stage rather than fail the import: raw page view always remains available, low-confidence objects are visibly flagged, and the failure reason is reported in plain language.

#### Scenario: Scanned PDF
- **WHEN** the user imports a scanned (image-only) PDF
- **THEN** the paper opens in raw view, the app explains that object extraction is limited and why, and no misleading unflagged objects are shown

#### Scenario: Equation extraction uncertainty
- **WHEN** an equation is extracted with confidence below the display threshold
- **THEN** the reader marks it as low-confidence and offers "view original region" alongside any AI explanation

### Requirement: Import sources
The system SHALL import papers from local file selection, drag-and-drop, and arXiv URL / DOI (fetching the PDF and metadata when network is available).

#### Scenario: arXiv URL import
- **WHEN** the user pastes an arXiv URL
- **THEN** the PDF and metadata (title, authors, abstract, identifiers) are fetched and ingestion starts, with a clear error path if the network is unavailable

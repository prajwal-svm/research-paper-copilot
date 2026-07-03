# contextual-chat

## ADDED Requirements

### Requirement: Object-anchored conversations
Every chat SHALL be anchored to an object (or ad-hoc selection) and persisted in the bundle's `chats/` directory keyed by object UUID. Reopening an object SHALL resume its conversation with full history. There SHALL be no global, unanchored chat in v1.

#### Scenario: Resume a conversation
- **WHEN** the user reopens Equation 12 three days after asking questions about it
- **THEN** the prior thread is shown and follow-up questions include that history in context

### Requirement: Structured context assembly
Prompts SHALL be assembled from the anchored object's extracted content, its relationships (e.g., the equation a figure depends on, the section it belongs to), and the object's own conversation history — not from naive whole-document chunk retrieval. Context size SHALL stay within a fixed token budget per request.

#### Scenario: Relationship-aware answer
- **WHEN** the user asks about Figure 5 which depends on Equation 12
- **THEN** the prompt includes Equation 12's content via the relationship link, and the answer can reference it correctly

### Requirement: Streaming with citations to the paper
Responses SHALL stream token-by-token with first token within 1.5 s plus provider latency, and SHALL cite the paper objects they rely on as clickable references that navigate the reader.

#### Scenario: Clickable grounding
- **WHEN** an answer references "as defined in Section 3.2"
- **THEN** that reference is a link that scrolls the reader to the Section 3.2 object

### Requirement: Honest failure behavior
On provider errors, rate limits, or network loss mid-stream, the partial response SHALL be preserved and clearly marked, with one-click retry; the conversation log SHALL never be corrupted or silently lost.

#### Scenario: Network drop mid-answer
- **WHEN** connectivity is lost while an answer is streaming
- **THEN** the partial text remains visible and labeled incomplete, and retry regenerates without duplicating the user's question

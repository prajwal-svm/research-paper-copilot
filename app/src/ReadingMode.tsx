import { useCallback, useEffect, useRef, useState } from "react";
import { invoke } from "@/platform";
import { listen } from "@/platform";
import {
  CheckIcon,
  ChevronRightIcon,
  ExternalLinkIcon,
  GraduationCapIcon,
  XCircleIcon,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Empty, EmptyDescription, EmptyHeader, EmptyTitle } from "@/components/ui/empty";
import {
  InputGroup,
  InputGroupAddon,
  InputGroupButton,
  InputGroupInput,
} from "@/components/ui/input-group";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Separator } from "@/components/ui/separator";
import { Skeleton } from "@/components/ui/skeleton";
import { Spinner } from "@/components/ui/spinner";
import { MessageResponse } from "@/components/ai-elements/message";
import ObjectLinkedText from "./ai/ObjectLinkedText";

interface LessonEntry {
  node: string;
  name: string;
  description?: string;
  object_ids: string[];
  mastered: boolean;
  low_confidence: boolean;
}

interface Lesson {
  node: string;
  content: string;
  exercise?: string;
}

interface QuizItem {
  id: string;
  question: string;
  options: string[];
  correct: number;
  explanation: string;
  anchor_object?: string;
  stale: boolean;
}

interface Quiz {
  node: string;
  items: QuizItem[];
}

interface Flashcard {
  id: string;
  front: string;
  back: string;
  anchor_object?: string;
  stale: boolean;
}

interface FlashcardDeck {
  node: string;
  cards: Flashcard[];
}

const cursorKey = (paperId: string) => `rpc-lesson-cursor-${paperId}`;

/**
 * Reading mode (v2): the paper as a course. Lessons follow the concept
 * graph's prerequisite topology; mastered lessons collapse to a recap but
 * stay one click away (mastery never gates). Content is generated lazily and
 * cached in the bundle — navigation never blocks on generation (skeleton
 * within 300 ms), and "show me in the paper" escapes to the raw paper with
 * the lesson cursor persisted for the return trip.
 */
export default function ReadingMode({
  paperId,
  labelFor,
  onEscapeToObject,
  onNavigateObject,
}: {
  paperId: string;
  labelFor: (objectId: string) => string | undefined;
  /** Escape hatch: leave reading mode and open the paper at this object. */
  onEscapeToObject: (objectId: string) => void;
  /** Follow an inline [[object:…]] reference without leaving the mode. */
  onNavigateObject: (objectId: string) => void;
}) {
  const [sequence, setSequence] = useState<LessonEntry[] | null>(null);
  const [dueNodes, setDueNodes] = useState<Set<string>>(new Set());
  const [error, setError] = useState<string | null>(null);
  const [cursor, setCursor] = useState<number>(() => {
    const saved = Number(localStorage.getItem(cursorKey(paperId)));
    return Number.isFinite(saved) && saved >= 0 ? saved : 0;
  });

  const refreshSequence = useCallback(() => {
    invoke<LessonEntry[]>("lessons_sequence", { paperId })
      .then(setSequence)
      .catch((e) => setError(String(e)));
    // Spaced repetition: concepts whose review interval has elapsed.
    invoke<LessonEntry[]>("review_due", { paperId })
      .then((due) => setDueNodes(new Set(due.map((d) => d.node))))
      .catch(() => {});
  }, [paperId]);
  useEffect(refreshSequence, [refreshSequence]);

  useEffect(() => {
    localStorage.setItem(cursorKey(paperId), String(cursor));
  }, [paperId, cursor]);

  if (error) {
    return (
      <div className="flex h-full items-center justify-center p-6">
        <Empty>
          <EmptyHeader>
            <EmptyTitle>Reading mode isn’t ready</EmptyTitle>
            <EmptyDescription>{error}</EmptyDescription>
          </EmptyHeader>
        </Empty>
      </div>
    );
  }
  if (!sequence) {
    return (
      <div className="flex h-full items-center justify-center">
        <Spinner />
      </div>
    );
  }
  if (sequence.length === 0) {
    return (
      <div className="flex h-full items-center justify-center p-6">
        <Empty>
          <EmptyHeader>
            <EmptyTitle>No concepts extracted yet</EmptyTitle>
            <EmptyDescription>
              The course outline comes from the paper’s concept graph. Reopen
              the paper after ingestion finishes.
            </EmptyDescription>
          </EmptyHeader>
        </Empty>
      </div>
    );
  }

  const active = sequence[Math.min(cursor, sequence.length - 1)];

  return (
    <div className="flex h-full min-h-0">
      {/* Course outline: topological order, mastered = collapsed recap. */}
      <ScrollArea className="w-60 flex-none border-r">
        <ol className="flex flex-col gap-0.5 p-2 pt-10">
          {sequence.map((entry, i) => (
            <li key={entry.node}>
              <button
                className={
                  "flex w-full items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm hover:bg-accent " +
                  (i === Math.min(cursor, sequence.length - 1) ? "bg-accent" : "") +
                  (entry.mastered ? " text-muted-foreground" : "")
                }
                onClick={() => setCursor(i)}
              >
                {entry.mastered ? (
                  <CheckIcon className="size-3.5 flex-none" />
                ) : (
                  <span className="w-3.5 flex-none text-xs tabular-nums">{i + 1}</span>
                )}
                <span className="truncate">{entry.name}</span>
                {dueNodes.has(entry.node) ? (
                  <Badge variant="secondary" className="ml-auto flex-none">
                    review due
                  </Badge>
                ) : (
                  entry.mastered && (
                    <Badge variant="outline" className="ml-auto flex-none">
                      recap
                    </Badge>
                  )
                )}
              </button>
            </li>
          ))}
        </ol>
      </ScrollArea>

      <LessonPlayer
        key={active.node}
        paperId={paperId}
        entry={active}
        labelFor={labelFor}
        onNavigateObject={onNavigateObject}
        onEscapeToObject={onEscapeToObject}
        onMasteryChanged={refreshSequence}
        onNext={cursor < sequence.length - 1 ? () => setCursor(cursor + 1) : undefined}
      />
    </div>
  );
}

// ---------------------------------------------------------------------------

function LessonPlayer({
  paperId,
  entry,
  labelFor,
  onNavigateObject,
  onEscapeToObject,
  onMasteryChanged,
  onNext,
}: {
  paperId: string;
  entry: LessonEntry;
  labelFor: (objectId: string) => string | undefined;
  onNavigateObject: (objectId: string) => void;
  onEscapeToObject: (objectId: string) => void;
  onMasteryChanged: () => void;
  onNext?: () => void;
}) {
  const [lesson, setLesson] = useState<Lesson | null>(null);
  const [generating, setGenerating] = useState(true);
  const [quiz, setQuiz] = useState<Quiz | null>(null);
  const [quizLoading, setQuizLoading] = useState(false);
  const [deck, setDeck] = useState<FlashcardDeck | null>(null);
  const [deckLoading, setDeckLoading] = useState(false);
  const [practiceNotice, setPracticeNotice] = useState<string | null>(null);

  // Lazy generation: cached lessons return instantly; first generation shows
  // a skeleton while navigation stays fully responsive.
  useEffect(() => {
    let alive = true;
    setLesson(null);
    setGenerating(true);
    invoke<Lesson | null>("lesson_get_or_generate", { paperId, node: entry.node })
      .then((l) => alive && setLesson(l))
      .catch(() => {})
      .finally(() => alive && setGenerating(false));
    return () => {
      alive = false;
    };
  }, [paperId, entry.node]);

  // Designed no-key state: cached items work without a provider; only new
  // generation needs one — and says so instead of failing silently.
  const NO_KEY_NOTICE =
    "No cached items yet — generating new ones needs an AI provider (Settings).";

  function loadQuiz() {
    setQuizLoading(true);
    setPracticeNotice(null);
    invoke<Quiz | null>("quiz_get_or_generate", { paperId, node: entry.node })
      .then((q) => {
        setQuiz(q);
        if (!q) setPracticeNotice(NO_KEY_NOTICE);
      })
      .catch((e) => setPracticeNotice(String(e)))
      .finally(() => setQuizLoading(false));
  }

  function loadDeck() {
    setDeckLoading(true);
    setPracticeNotice(null);
    invoke<FlashcardDeck | null>("flashcards_get_or_generate", { paperId, node: entry.node })
      .then((d) => {
        setDeck(d);
        if (!d) setPracticeNotice(NO_KEY_NOTICE);
      })
      .catch((e) => setPracticeNotice(String(e)))
      .finally(() => setDeckLoading(false));
  }

  const anchor = entry.object_ids[0];

  return (
    <ScrollArea className="min-h-0 flex-1">
      <div className="mx-auto flex max-w-2xl flex-col gap-4 p-6 pt-10">
        <div className="flex items-center gap-2">
          <GraduationCapIcon className="text-muted-foreground size-4" />
          <h2 className="flex-1 truncate text-base font-semibold">{entry.name}</h2>
          {entry.mastered && <Badge variant="secondary">mastered — recap</Badge>}
          {anchor && (
            <Button variant="outline" size="sm" onClick={() => onEscapeToObject(anchor)}>
              <ExternalLinkIcon data-icon="inline-start" />
              Show me in the paper
            </Button>
          )}
        </div>

        {lesson ? (
          <ObjectLinkedText
            text={lesson.content}
            labelFor={labelFor}
            onNavigate={onNavigateObject}
          />
        ) : generating ? (
          <div className="flex flex-col gap-2">
            <Skeleton className="h-4 w-3/4" />
            <Skeleton className="h-4 w-full" />
            <Skeleton className="h-4 w-5/6" />
            <Skeleton className="h-4 w-2/3" />
          </div>
        ) : (
          // Designed no-key state: the paper's own material, never an error.
          <div className="flex flex-col gap-2">
            <p className="text-muted-foreground text-sm">
              Lesson generation needs an AI provider (Settings). The paper’s
              own material for this concept is right here:
            </p>
            <div className="flex flex-wrap gap-1.5">
              {entry.object_ids.map((id) => (
                <Button key={id} variant="outline" size="sm" onClick={() => onEscapeToObject(id)}>
                  {labelFor(id) ?? "Open in paper"}
                </Button>
              ))}
            </div>
            {entry.description && (
              <p className="text-sm">{entry.description}</p>
            )}
          </div>
        )}

        {lesson && (
          <>
            <Separator />
            <TutorPanel paperId={paperId} entry={entry} onMasteryChanged={onMasteryChanged} />
            <Separator />
            {quiz ? (
              <QuizBlock
                paperId={paperId}
                entry={entry}
                quiz={quiz}
                labelFor={labelFor}
                onEscapeToObject={onEscapeToObject}
                onMasteryChanged={onMasteryChanged}
              />
            ) : deck ? (
              <FlashcardBlock
                paperId={paperId}
                entry={entry}
                deck={deck}
                onMasteryChanged={onMasteryChanged}
              />
            ) : (
              <div className="flex flex-col gap-2">
                <div className="flex gap-2">
                  <Button variant="outline" disabled={quizLoading || deckLoading} onClick={loadQuiz}>
                    {quizLoading && <Spinner data-icon="inline-start" />}
                    Quiz me
                  </Button>
                  <Button variant="outline" disabled={quizLoading || deckLoading} onClick={loadDeck}>
                    {deckLoading && <Spinner data-icon="inline-start" />}
                    Flashcards
                  </Button>
                </div>
                {practiceNotice && (
                  <p className="text-muted-foreground text-xs">{practiceNotice}</p>
                )}
              </div>
            )}
          </>
        )}

        {onNext && (
          <Button className="self-end" onClick={onNext}>
            Next lesson
            <ChevronRightIcon data-icon="inline-end" />
          </Button>
        )}
      </div>
    </ScrollArea>
  );
}

// ---------------------------------------------------------------------------

function QuizBlock({
  paperId,
  entry,
  quiz,
  labelFor,
  onEscapeToObject,
  onMasteryChanged,
}: {
  paperId: string;
  entry: LessonEntry;
  quiz: Quiz;
  labelFor: (objectId: string) => string | undefined;
  onEscapeToObject: (objectId: string) => void;
  onMasteryChanged: () => void;
}) {
  // item id → chosen option index
  const [answers, setAnswers] = useState<Record<string, number>>({});

  function answer(item: QuizItem, choice: number) {
    if (answers[item.id] !== undefined) return;
    setAnswers((a) => ({ ...a, [item.id]: choice }));
    const correct = choice === item.correct;
    // One data path: this event drives mastery, dashboard, lesson collapse.
    invoke("learning_record", {
      paperId,
      node: entry.node,
      quality: correct ? 5 : 1,
      source: "quiz",
      object: item.anchor_object ?? null,
    })
      .then(onMasteryChanged)
      .catch(() => {});
  }

  return (
    <div className="flex flex-col gap-4">
      {quiz.items.map((item, qi) => {
        const chosen = answers[item.id];
        return (
          <div key={item.id} className="flex flex-col gap-2">
            <p className="text-sm font-medium">
              {qi + 1}. {item.question}
              {item.stale && (
                <Badge variant="outline" className="ml-2">
                  may be outdated — paper re-parsed
                </Badge>
              )}
            </p>
            <div className="flex flex-col gap-1.5">
              {item.options.map((option, oi) => {
                const isChosen = chosen === oi;
                const answered = chosen !== undefined;
                const isCorrect = oi === item.correct;
                return (
                  <Button
                    key={oi}
                    variant={answered && (isChosen || isCorrect) ? "secondary" : "outline"}
                    size="sm"
                    className="h-auto justify-start whitespace-normal py-1.5 text-left"
                    disabled={answered}
                    onClick={() => answer(item, oi)}
                  >
                    {answered && isCorrect && <CheckIcon data-icon="inline-start" />}
                    {answered && isChosen && !isCorrect && (
                      <XCircleIcon data-icon="inline-start" />
                    )}
                    {option}
                  </Button>
                );
              })}
            </div>
            {chosen !== undefined && (
              <div className="text-muted-foreground text-sm">
                <MessageResponse>{item.explanation}</MessageResponse>
                {item.anchor_object && (
                  <Button
                    variant="link"
                    size="sm"
                    className="h-auto p-0"
                    onClick={() => onEscapeToObject(item.anchor_object!)}
                  >
                    {labelFor(item.anchor_object) ?? "See it in the paper"}
                  </Button>
                )}
              </div>
            )}
          </div>
        );
      })}
    </div>
  );
}

// ---------------------------------------------------------------------------

/**
 * Flashcard review: flip, then self-grade. Grades map onto the SM-2 quality
 * scale, so a failed card comes due again sooner than a passed one — same
 * mastery events the dashboard and lesson collapsing read.
 */
function FlashcardBlock({
  paperId,
  entry,
  deck,
  onMasteryChanged,
}: {
  paperId: string;
  entry: LessonEntry;
  deck: FlashcardDeck;
  onMasteryChanged: () => void;
}) {
  const [index, setIndex] = useState(0);
  const [flipped, setFlipped] = useState(false);

  if (index >= deck.cards.length) {
    return <p className="text-muted-foreground text-sm">Deck done — nice work.</p>;
  }
  const card = deck.cards[index];

  function grade(quality: number) {
    invoke("learning_record", {
      paperId,
      node: entry.node,
      quality,
      source: "flashcard",
      object: card.anchor_object ?? null,
    })
      .then(onMasteryChanged)
      .catch(() => {});
    setFlipped(false);
    setIndex((i) => i + 1);
  }

  return (
    <div className="flex flex-col gap-3">
      <p className="text-muted-foreground text-xs">
        Card {index + 1} of {deck.cards.length}
        {card.stale && " — may be outdated (paper re-parsed)"}
      </p>
      <button
        className="hover:bg-accent min-h-24 rounded-lg border p-4 text-left text-sm"
        onClick={() => setFlipped((f) => !f)}
      >
        {flipped ? card.back : card.front}
        {!flipped && (
          <span className="text-muted-foreground mt-2 block text-xs">tap to reveal</span>
        )}
      </button>
      {flipped && (
        <div className="flex gap-1.5">
          <Button variant="outline" size="sm" onClick={() => grade(1)}>
            Again
          </Button>
          <Button variant="outline" size="sm" onClick={() => grade(3)}>
            Hard
          </Button>
          <Button variant="outline" size="sm" onClick={() => grade(4)}>
            Good
          </Button>
          <Button variant="outline" size="sm" onClick={() => grade(5)}>
            Easy
          </Button>
        </div>
      )}
    </div>
  );
}

// ---------------------------------------------------------------------------

type TutorPhase = "idle" | "streaming" | "waiting" | "done";

/**
 * Socratic tutor (v2): client-enforced state machine over streaming chat.
 * ask → wait for the attempt → hint ladder (model judges, [CORRECT] token
 * signals success) → correction on request or after 3 hints. Outcomes append
 * mastery events; the loop never traps the user.
 */
function TutorPanel({
  paperId,
  entry,
  onMasteryChanged,
}: {
  paperId: string;
  entry: LessonEntry;
  onMasteryChanged: () => void;
}) {
  const [phase, setPhase] = useState<TutorPhase>("idle");
  const [transcript, setTranscript] = useState<{ role: string; text: string }[]>([]);
  const [streamText, setStreamText] = useState("");
  const [attempt, setAttempt] = useState("");
  const [hintsUsed, setHintsUsed] = useState(0);
  const [error, setError] = useState<string | null>(null);
  const activeRequest = useRef<string | null>(null);

  useEffect(() => {
    const unlisten = listen<{
      request_id: string;
      token?: string;
      done?: boolean;
      error?: string;
      cancelled?: boolean;
    }>("ai-stream", ({ payload }) => {
      if (payload.request_id !== activeRequest.current) return;
      if (payload.token) setStreamText((t) => t + payload.token);
      if (payload.error) {
        setError(payload.error);
        setPhase("waiting");
      }
    });
    return () => {
      unlisten.then((fn) => fn());
    };
  }, []);

  function record(quality: number) {
    invoke("learning_record", {
      paperId,
      node: entry.node,
      quality,
      source: "tutor",
      object: entry.object_ids[0] ?? null,
    })
      .then(onMasteryChanged)
      .catch(() => {});
  }

  async function turn(tutorPhase: "ask" | "hint" | "correct", userAttempt?: string) {
    const requestId = crypto.randomUUID();
    activeRequest.current = requestId;
    setError(null);
    setStreamText("");
    setPhase("streaming");
    if (userAttempt) {
      setTranscript((t) => [...t, { role: "user", text: userAttempt }]);
    }
    try {
      const full = await invoke<string>("tutor_stream", {
        requestId,
        paperId,
        node: entry.node,
        phase: tutorPhase,
        attempt: userAttempt ?? null,
        hintsUsed,
      });
      const correct = tutorPhase === "hint" && full.includes("[CORRECT]");
      const clean = full.replace("[CORRECT]", "").trim();
      setTranscript((t) => [...t, { role: "assistant", text: clean }]);
      setStreamText("");
      if (correct) {
        record(Math.max(3, 5 - hintsUsed));
        setPhase("done");
      } else if (tutorPhase === "correct") {
        record(2); // needed the correction — honest signal, not a punishment
        setPhase("done");
      } else {
        if (tutorPhase === "hint") setHintsUsed((h) => h + 1);
        setPhase("waiting");
      }
    } catch (e) {
      setError(String(e));
      setStreamText("");
      setPhase(transcript.length === 0 ? "idle" : "waiting");
    }
  }

  function submitAttempt() {
    const text = attempt.trim();
    if (!text) return;
    setAttempt("");
    // 3 hints exhausted → the next attempt gets the correction either way.
    turn(hintsUsed >= 3 ? "correct" : "hint", text);
  }

  if (phase === "idle") {
    return (
      <div className="flex items-center gap-3">
        <Button variant="outline" onClick={() => turn("ask")}>
          <GraduationCapIcon data-icon="inline-start" />
          Practice with the tutor
        </Button>
        {error && <span className="text-muted-foreground text-xs">{error}</span>}
      </div>
    );
  }

  return (
    <div className="flex flex-col gap-2">
      {transcript.map((turn, i) =>
        turn.role === "user" ? (
          <p key={i} className="text-muted-foreground pl-4 text-sm">
            You: {turn.text}
          </p>
        ) : (
          <div key={i} className="text-sm">
            <MessageResponse>{turn.text}</MessageResponse>
          </div>
        ),
      )}
      {streamText && (
        <div className="text-sm">
          <MessageResponse>{streamText.replace("[CORRECT]", "")}</MessageResponse>
        </div>
      )}
      {phase === "streaming" && !streamText && <Skeleton className="h-4 w-2/3" />}
      {error && <p className="text-destructive text-xs">{error}</p>}

      {phase === "waiting" && (
        <div className="flex flex-col gap-1.5">
          <InputGroup>
            <InputGroupInput
              placeholder="Your answer…"
              value={attempt}
              onChange={(e) => setAttempt(e.target.value)}
              onKeyDown={(e) => e.key === "Enter" && submitAttempt()}
              autoFocus
            />
            <InputGroupAddon align="inline-end">
              <InputGroupButton size="sm" onClick={submitAttempt} disabled={!attempt.trim()}>
                Answer
              </InputGroupButton>
            </InputGroupAddon>
          </InputGroup>
          <Button
            variant="ghost"
            size="sm"
            className="self-start"
            onClick={() => turn("correct")}
          >
            Just tell me
          </Button>
        </div>
      )}
      {phase === "done" && (
        <Button
          variant="outline"
          size="sm"
          className="self-start"
          onClick={() => {
            setHintsUsed(0);
            turn("ask");
          }}
        >
          Another question
        </Button>
      )}
    </div>
  );
}

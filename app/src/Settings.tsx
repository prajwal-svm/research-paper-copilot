import { useEffect, useState } from "react";
import { invoke } from "@/platform";
import {
  BotIcon,
  ChartNoAxesColumnIcon,
  CheckIcon,
  CloudIcon,
  GraduationCapIcon,
  HardDriveIcon,
  SettingsIcon,
  SlidersHorizontalIcon,
  Trash2Icon,
} from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import {
  Dialog,
  DialogContent,
  DialogDescription,
  DialogHeader,
  DialogTitle,
  DialogTrigger,
} from "@/components/ui/dialog";
import { Field, FieldDescription, FieldGroup, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { ScrollArea } from "@/components/ui/scroll-area";
import { Spinner } from "@/components/ui/spinner";
import { Switch } from "@/components/ui/switch";
import DiskHygiene from "./DiskHygiene";
import LearningData from "./LearningData";
import ProviderPresets from "./ProviderPresets";
import SyncSettings from "./SyncSettings";

type ProviderKind = "anthropic" | "open_ai" | "open_router" | "ollama";

interface ProviderStatus {
  kind: ProviderKind;
  has_key: boolean;
  available: boolean;
}

const PROVIDER_LABEL: Record<ProviderKind, string> = {
  anthropic: "Anthropic",
  open_ai: "OpenAI",
  open_router: "OpenRouter",
  ollama: "Ollama (local)",
};

type Section = "providers" | "presets" | "sync" | "learning" | "storage" | "privacy";

const SECTIONS: { id: Section; label: string; icon: React.ComponentType }[] = [
  { id: "providers", label: "AI providers", icon: BotIcon },
  { id: "presets", label: "Presets & models", icon: SlidersHorizontalIcon },
  { id: "sync", label: "Sync", icon: CloudIcon },
  { id: "learning", label: "Learning data", icon: GraduationCapIcon },
  { id: "storage", label: "Storage", icon: HardDriveIcon },
  { id: "privacy", label: "Privacy", icon: ChartNoAxesColumnIcon },
];

const SECTION_BLURB: Record<Section, string> = {
  providers:
    "Bring your own key, or run models locally with Ollama. Keys are validated with a test call and stored in your OS keychain.",
  presets: "Provider presets, custom endpoints, and per-tier model routing.",
  sync: "Your library on every device — bring your own storage, end-to-end encrypted.",
  learning: "What the app has learned about your understanding — local, inspectable, deletable.",
  storage: "Caches that are safe to clear.",
  privacy: "Opt-in, content-free usage statistics. Nothing is transmitted anywhere.",
};

/** Settings: a sidebar of sections on the left, the active section on the
 * right. All data stays local except what each section explicitly sends. */
export default function Settings({
  open: controlledOpen,
  onOpenChange,
  showTrigger = true,
}: {
  /** Controlled open (omnibar); omit for the self-contained trigger button. */
  open?: boolean;
  onOpenChange?: (open: boolean) => void;
  showTrigger?: boolean;
} = {}) {
  const [uncontrolledOpen, setUncontrolledOpen] = useState(false);
  const open = controlledOpen ?? uncontrolledOpen;
  const setOpen = onOpenChange ?? setUncontrolledOpen;
  const [section, setSection] = useState<Section>("providers");

  return (
    <Dialog open={open} onOpenChange={setOpen}>
      {showTrigger && (
        <DialogTrigger asChild>
          <Button variant="ghost" size="icon" title="Settings">
            <SettingsIcon />
          </Button>
        </DialogTrigger>
      )}
      <DialogContent className="h-[70vh] overflow-hidden p-0 sm:max-w-4xl">
        <div className="flex h-full min-h-0">
          {/* Sidebar */}
          <nav className="bg-muted/40 flex w-52 flex-none flex-col gap-0.5 border-r p-3 pt-5">
            <h2 className="text-muted-foreground mb-2 px-2 text-xs font-medium uppercase tracking-wide">
              Settings
            </h2>
            {SECTIONS.map(({ id, label, icon: Icon }) => (
              <button
                key={id}
                className={
                  "flex items-center gap-2 rounded-md px-2 py-1.5 text-left text-sm hover:bg-accent [&_svg]:size-4 [&_svg]:flex-none " +
                  (section === id ? "bg-accent font-medium" : "text-muted-foreground")
                }
                onClick={() => setSection(id)}
              >
                <Icon />
                {label}
              </button>
            ))}
          </nav>

          {/* Active section */}
          <ScrollArea className="min-h-0 flex-1">
            <div className="flex flex-col gap-4 p-6">
              <DialogHeader>
                <DialogTitle>{SECTIONS.find((s) => s.id === section)?.label}</DialogTitle>
                <DialogDescription>{SECTION_BLURB[section]}</DialogDescription>
              </DialogHeader>
              {section === "providers" && open && <ProvidersSection />}
              {section === "presets" && <ProviderPresets />}
              {section === "sync" && <SyncSettings />}
              {section === "learning" && <LearningData />}
              {section === "storage" && <DiskHygiene />}
              {section === "privacy" && open && <PrivacySection />}
            </div>
          </ScrollArea>
        </div>
      </DialogContent>
    </Dialog>
  );
}

// ---------------------------------------------------------------------------

function ProvidersSection() {
  const [statuses, setStatuses] = useState<ProviderStatus[]>([]);
  const [drafts, setDrafts] = useState<Partial<Record<ProviderKind, string>>>({});
  const [busyKind, setBusyKind] = useState<ProviderKind | null>(null);
  const [messages, setMessages] = useState<Partial<Record<ProviderKind, string>>>({});

  const refresh = () => {
    invoke<ProviderStatus[]>("ai_provider_statuses").then(setStatuses).catch(() => {});
  };
  useEffect(refresh, []);

  async function saveKey(kind: ProviderKind) {
    const key = drafts[kind]?.trim();
    if (!key) return;
    setBusyKind(kind);
    setMessages((m) => ({ ...m, [kind]: undefined }));
    try {
      const summary = await invoke<string>("ai_set_key", { kind, key });
      setMessages((m) => ({ ...m, [kind]: summary }));
      setDrafts((d) => ({ ...d, [kind]: "" }));
      refresh();
    } catch (e) {
      setMessages((m) => ({ ...m, [kind]: String(e) }));
    } finally {
      setBusyKind(null);
    }
  }

  async function removeKey(kind: ProviderKind) {
    try {
      await invoke("ai_delete_key", { kind });
      refresh();
    } catch (e) {
      setMessages((m) => ({ ...m, [kind]: String(e) }));
    }
  }

  return (
    <FieldGroup>
      {statuses.map((status) => (
        <Field key={status.kind}>
          <FieldLabel htmlFor={`key-${status.kind}`}>
            <span className="flex items-center gap-2">
              {PROVIDER_LABEL[status.kind]}
              {status.available && (
                <Badge variant="secondary">
                  <CheckIcon data-icon="inline-start" />
                  {status.kind === "ollama" ? "running" : "configured"}
                </Badge>
              )}
            </span>
          </FieldLabel>
          {status.kind !== "ollama" ? (
            <div className="flex items-center gap-2">
              <Input
                id={`key-${status.kind}`}
                type="password"
                placeholder={status.has_key ? "•••••••• (stored)" : "API key"}
                value={drafts[status.kind] ?? ""}
                onChange={(e) => setDrafts((d) => ({ ...d, [status.kind]: e.target.value }))}
                onKeyDown={(e) => e.key === "Enter" && saveKey(status.kind)}
              />
              <Button
                size="sm"
                onClick={() => saveKey(status.kind)}
                disabled={busyKind === status.kind || !drafts[status.kind]?.trim()}
              >
                {busyKind === status.kind && <Spinner data-icon="inline-start" />}
                Save
              </Button>
              {status.has_key && (
                <Button
                  variant="ghost"
                  size="icon-sm"
                  title="Remove key"
                  onClick={() => removeKey(status.kind)}
                >
                  <Trash2Icon />
                </Button>
              )}
            </div>
          ) : (
            <FieldDescription>
              {status.available
                ? "Detected on localhost:11434 — no key needed."
                : "Not detected. Install and start Ollama to use local models."}
            </FieldDescription>
          )}
          {messages[status.kind] && <FieldDescription>{messages[status.kind]}</FieldDescription>}
        </Field>
      ))}
    </FieldGroup>
  );
}

function PrivacySection() {
  const [enabled, setEnabled] = useState(false);

  useEffect(() => {
    invoke<{ enabled: boolean }>("telemetry_summary")
      .then((s) => setEnabled(s.enabled))
      .catch(() => {});
  }, []);

  async function toggle(next: boolean) {
    setEnabled(next);
    await invoke("telemetry_set_enabled", { enabled: next }).catch(() => {});
  }

  return (
    <Field orientation="horizontal">
      <FieldLabel htmlFor="telemetry-toggle">Usage statistics</FieldLabel>
      <Switch id="telemetry-toggle" checked={enabled} onCheckedChange={toggle} />
      <FieldDescription>
        Off by default. When on, anonymous counters (session health, feature
        usage, answer thumbs) are stored locally on this machine — never paper
        content, notes, or questions, and nothing is transmitted anywhere.
      </FieldDescription>
    </Field>
  );
}

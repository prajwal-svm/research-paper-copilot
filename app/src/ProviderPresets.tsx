import { useCallback, useEffect, useState } from "react";
import { invoke } from "@/platform";
import { GlobeIcon, PlusIcon, Trash2Icon, Undo2Icon } from "lucide-react";
import { Badge } from "@/components/ui/badge";
import { Button } from "@/components/ui/button";
import { Field, FieldDescription, FieldLabel } from "@/components/ui/field";
import { Input } from "@/components/ui/input";
import { Spinner } from "@/components/ui/spinner";
import { Switch } from "@/components/ui/switch";

interface TierModels {
  strong: string;
  balanced: string;
  light: string;
}

export interface ProviderConfig {
  id: string;
  name: string;
  protocol: string;
  base_url: string;
  models: TierModels;
  request_timeout_ms: number;
  one_m_context: boolean;
  preset_id?: string | null;
}

interface ProviderConfigView extends ProviderConfig {
  has_key: boolean;
  host: string;
  is_custom_url: boolean;
  preset_defaults?: {
    models: TierModels;
    request_timeout_ms: number;
    supports_one_m: boolean;
    base_url: string;
  } | null;
}

interface ProviderPreset extends ProviderConfig {
  supports_one_m: boolean;
  docs_url?: string;
}

/**
 * Preset providers (e.g. Z.ai GLM Coding Plan) + custom Anthropic-compatible
 * endpoints. The host paper content goes to is always visible; user-entered
 * URLs are visually distinct from shipped presets (trust styling).
 */
export default function ProviderPresets() {
  const [configs, setConfigs] = useState<ProviderConfigView[]>([]);
  const [presets, setPresets] = useState<ProviderPreset[]>([]);
  const [draftKeys, setDraftKeys] = useState<Record<string, string>>({});
  const [busyId, setBusyId] = useState<string | null>(null);
  const [messages, setMessages] = useState<Record<string, string>>({});

  const refresh = useCallback(() => {
    invoke<ProviderConfigView[]>("provider_configs").then(setConfigs).catch(() => {});
    invoke<ProviderPreset[]>("provider_presets").then(setPresets).catch(() => {});
  }, []);

  useEffect(refresh, [refresh]);

  const configuredIds = new Set(configs.map((c) => c.id));
  const addablePresets = presets.filter((p) => !configuredIds.has(p.id));

  async function save(config: ProviderConfig, key?: string) {
    setBusyId(config.id);
    setMessages((m) => ({ ...m, [config.id]: "" }));
    try {
      const summary = await invoke<string>("save_provider_config", {
        config,
        key: key || null,
      });
      setMessages((m) => ({ ...m, [config.id]: summary }));
      setDraftKeys((d) => ({ ...d, [config.id]: "" }));
      refresh();
    } catch (e) {
      // Validation failed → nothing was saved (no partial configuration).
      setMessages((m) => ({ ...m, [config.id]: String(e) }));
    } finally {
      setBusyId(null);
    }
  }

  async function addPreset(preset: ProviderPreset) {
    const config: ProviderConfig = {
      id: preset.id,
      name: preset.name,
      protocol: preset.protocol,
      base_url: preset.base_url,
      models: preset.models,
      request_timeout_ms: preset.request_timeout_ms,
      one_m_context: false,
      preset_id: preset.id,
    };
    // Persist without a key first so the key field appears; validation
    // happens when the key is saved.
    await save(config);
  }

  async function remove(id: string) {
    await invoke("remove_provider_config", { id }).catch(() => {});
    refresh();
  }

  const presetConfigs = configs.filter((c) => c.preset_id);

  return (
    <div className="flex flex-col gap-4">
      <div className="flex items-center justify-between">
        <span className="text-sm font-medium">Provider presets</span>
        {addablePresets.map((preset) => (
          <Button key={preset.id} variant="outline" size="sm" onClick={() => addPreset(preset)}>
            <PlusIcon data-icon="inline-start" />
            {preset.name}
          </Button>
        ))}
      </div>

      {presetConfigs.map((config) => (
        <div key={config.id} className="flex flex-col gap-2 rounded-lg border p-3">
          <div className="flex items-center justify-between gap-2">
            <div className="flex min-w-0 items-center gap-2">
              <span className="truncate text-sm font-medium">{config.name}</span>
              <Badge variant={config.is_custom_url ? "destructive" : "secondary"}>
                <GlobeIcon data-icon="inline-start" />
                {config.host}
              </Badge>
              {config.has_key && <Badge variant="outline">key stored</Badge>}
            </div>
            <Button
              variant="ghost"
              size="icon-sm"
              title="Remove this provider"
              onClick={() => remove(config.id)}
            >
              <Trash2Icon />
            </Button>
          </div>

          {/* Key: validated against the endpoint before anything is saved. */}
          <div className="flex items-center gap-2">
            <Input
              type="password"
              placeholder={config.has_key ? "•••••••• (stored)" : "API key"}
              value={draftKeys[config.id] ?? ""}
              onChange={(e) => setDraftKeys((d) => ({ ...d, [config.id]: e.target.value }))}
              onKeyDown={(e) =>
                e.key === "Enter" && save(config, draftKeys[config.id]?.trim())
              }
            />
            <Button
              size="sm"
              disabled={busyId === config.id || !draftKeys[config.id]?.trim()}
              onClick={() => save(config, draftKeys[config.id]?.trim())}
            >
              {busyId === config.id && <Spinner data-icon="inline-start" />}
              Validate & save
            </Button>
          </div>

          {/* Tier → model mapping, editable so new models need no release. */}
          <div className="grid grid-cols-3 gap-2">
            {(["strong", "balanced", "light"] as const).map((tier) => (
              <Field key={tier}>
                <FieldLabel htmlFor={`${config.id}-${tier}`}>{tier}</FieldLabel>
                <Input
                  id={`${config.id}-${tier}`}
                  value={config.models[tier]}
                  onChange={(e) =>
                    setConfigs((all) =>
                      all.map((c) =>
                        c.id === config.id
                          ? { ...c, models: { ...c.models, [tier]: e.target.value } }
                          : c,
                      ),
                    )
                  }
                  onBlur={() => save(stripView(config))}
                />
              </Field>
            ))}
          </div>

          <div className="flex flex-wrap items-center gap-4">
            {config.preset_defaults?.supports_one_m && (
              <label className="flex cursor-pointer items-center gap-2 text-sm">
                <Switch
                  checked={config.one_m_context}
                  onCheckedChange={(checked) =>
                    save({ ...stripView(config), one_m_context: checked })
                  }
                />
                1M context
              </label>
            )}
            <label className="flex items-center gap-2 text-sm">
              Timeout (ms)
              <Input
                className="w-28"
                type="number"
                value={config.request_timeout_ms}
                onChange={(e) =>
                  setConfigs((all) =>
                    all.map((c) =>
                      c.id === config.id
                        ? { ...c, request_timeout_ms: Number(e.target.value) || 300000 }
                        : c,
                    ),
                  )
                }
                onBlur={() => save(stripView(config))}
              />
            </label>
            {config.preset_defaults && (
              <Button
                variant="ghost"
                size="sm"
                title="Revert models and timeout to the preset defaults"
                onClick={() =>
                  save({
                    ...stripView(config),
                    models: config.preset_defaults!.models,
                    request_timeout_ms: config.preset_defaults!.request_timeout_ms,
                    base_url: config.preset_defaults!.base_url,
                  })
                }
              >
                <Undo2Icon data-icon="inline-start" />
                Revert to preset
              </Button>
            )}
          </div>

          {messages[config.id] && (
            <FieldDescription>{messages[config.id]}</FieldDescription>
          )}
        </div>
      ))}

      {/* Custom Anthropic-compatible endpoint on the first-party Anthropic
          provider. User-entered URLs get trust styling: the host is always
          shown, marked as custom. */}
      <AnthropicBaseUrl
        configs={configs}
        busy={busyId === "anthropic"}
        message={messages["anthropic"] ?? ""}
        onSave={save}
      />
    </div>
  );
}

function stripView(view: ProviderConfigView): ProviderConfig {
  const { id, name, protocol, base_url, models, request_timeout_ms, one_m_context, preset_id } =
    view;
  return { id, name, protocol, base_url, models, request_timeout_ms, one_m_context, preset_id };
}

function AnthropicBaseUrl({
  configs,
  busy,
  message,
  onSave,
}: {
  configs: ProviderConfigView[];
  busy: boolean;
  message: string;
  onSave: (config: ProviderConfig, key?: string) => void;
}) {
  const anthropic = configs.find((c) => c.id === "anthropic");
  const [url, setUrl] = useState<string | null>(null);
  const [key, setKey] = useState("");
  if (!anthropic) return null;
  const value = url ?? anthropic.base_url;

  return (
    <div className="flex flex-col gap-2">
      <Field>
        <FieldLabel htmlFor="anthropic-base-url">
          <span className="flex items-center gap-2">
            Anthropic base URL (advanced)
            {anthropic.is_custom_url && (
              <Badge variant="destructive">
                <GlobeIcon data-icon="inline-start" />
                custom: {anthropic.host}
              </Badge>
            )}
          </span>
        </FieldLabel>
        <div className="flex items-center gap-2">
          <Input
            id="anthropic-base-url"
            value={value}
            onChange={(e) => setUrl(e.target.value)}
            placeholder="https://api.anthropic.com"
          />
          <Input
            className="w-40"
            type="password"
            placeholder="key (to validate)"
            value={key}
            onChange={(e) => setKey(e.target.value)}
          />
          <Button
            size="sm"
            disabled={busy || !key.trim() || value === anthropic.base_url}
            onClick={() =>
              onSave({ ...stripView(anthropic), base_url: value.trim() }, key.trim())
            }
          >
            {busy && <Spinner data-icon="inline-start" />}
            Validate & save
          </Button>
        </div>
        <FieldDescription>
          Any Anthropic-compatible endpoint. Paper content will be sent to the
          host shown above — change this only if you trust that server. The
          key is validated against the new endpoint before anything is saved.
        </FieldDescription>
      </Field>
      {message && <FieldDescription>{message}</FieldDescription>}
    </div>
  );
}

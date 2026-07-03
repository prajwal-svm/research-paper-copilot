import { useEffect, useState } from "react";
import { capabilities, platform, type Capability } from "@/platform";
import { Alert, AlertDescription, AlertTitle } from "@/components/ui/alert";

/** Explicit degradation notice (v5 capability matrix): on web, features
 * that need native capabilities or a runner explain themselves — they
 * never silently disappear or half-work. Renders nothing on desktop. */
export default function CapabilityBanner({ id }: { id: string }) {
  const [capability, setCapability] = useState<Capability | null>(null);

  useEffect(() => {
    if (platform === "desktop") return;
    capabilities().then((matrix) => {
      const found = matrix.find((c) => c.id === id);
      if (found && found.availability !== "web") setCapability(found);
    });
  }, [id]);

  if (!capability) return null;
  return (
    <Alert className="m-3">
      <AlertTitle>
        {capability.label} —{" "}
        {capability.availability === "web_via_runner"
          ? "needs the desktop app or a runner"
          : "desktop only"}
      </AlertTitle>
      <AlertDescription>
        {capability.web_note} See docs/guides/web.md for runner setup.
      </AlertDescription>
    </Alert>
  );
}

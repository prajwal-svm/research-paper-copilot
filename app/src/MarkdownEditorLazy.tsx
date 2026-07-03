import { lazy, Suspense, type ComponentProps } from "react";
import { Skeleton } from "@/components/ui/skeleton";

// BlockNote is heavy; load it only when someone actually starts editing.
const Editor = lazy(() => import("./MarkdownEditor"));

export default function MarkdownEditorLazy(props: ComponentProps<typeof Editor>) {
  return (
    <Suspense fallback={<Skeleton className="h-24 w-full rounded-md" />}>
      <Editor {...props} />
    </Suspense>
  );
}

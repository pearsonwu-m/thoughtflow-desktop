import { useState } from "react";
import { displayAccelerator, eventToAccelerator } from "../lib/shortcuts";

interface Props {
  value: string;
  label: string;
  /** Global shortcuts must include ⌘, ⌃, or ⌥ so they don't swallow typing. */
  requireModifier: boolean;
  onChange: (accelerator: string) => void;
}

/** Click (or press ↵), then press the new key combination. Esc cancels. */
export function ShortcutRecorder({ value, label, requireModifier, onChange }: Props) {
  const [recording, setRecording] = useState(false);
  const [hint, setHint] = useState<string | null>(null);

  return (
    <button
      className={`recorder${recording ? " recorder-on" : ""}`}
      aria-label={`${label}: ${displayAccelerator(value)}. Press to change.`}
      onClick={() => {
        setRecording(true);
        setHint(null);
      }}
      onBlur={() => setRecording(false)}
      onKeyDown={(e) => {
        if (!recording) return;
        e.preventDefault();
        e.stopPropagation();
        const bare = !(e.metaKey || e.ctrlKey || e.altKey || e.shiftKey);
        if (e.code === "Escape" && bare && requireModifier) {
          setRecording(false);
          return;
        }
        const accelerator = eventToAccelerator(e.nativeEvent, requireModifier);
        if (!accelerator) {
          if (!["Meta", "Control", "Alt", "Shift"].includes(e.key)) {
            setHint("Include ⌘, ⌃, or ⌥");
          }
          return;
        }
        if (!requireModifier && bare && e.code !== "Escape" && !/^F\d+$/.test(e.code)) {
          setHint("Include ⌘, ⌃, or ⌥");
          return;
        }
        setRecording(false);
        onChange(accelerator);
      }}
    >
      {recording ? (hint ?? "Press keys…") : displayAccelerator(value)}
    </button>
  );
}

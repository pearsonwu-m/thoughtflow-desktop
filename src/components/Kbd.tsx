import { displayAccelerator } from "../lib/shortcuts";

/** A subtle keyboard hint, e.g. <Kbd keys="CmdOrCtrl+Enter" /> → ⌘↵. */
export function Kbd({ keys }: { keys: string | undefined }) {
  if (!keys) return null;
  return <kbd aria-hidden="true">{displayAccelerator(keys)}</kbd>;
}

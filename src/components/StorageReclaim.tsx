import { useRef, useState } from "preact/hooks";
import { reclaimOpenCodeStorage } from "../api";
import { strings, type Locale } from "../i18n";
import type { StorageCleanupResult } from "../types";

export function StorageReclaim({ locale, disabled, onComplete }: {
  locale: Locale;
  disabled: boolean;
  onComplete: (result: StorageCleanupResult) => Promise<void>;
}) {
  const dialog = useRef<HTMLDialogElement>(null);
  const [working, setWorking] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const t = strings[locale];
  const reclaim = async () => {
    if (working) return;
    setWorking(true);
    setError(null);
    try {
      const result = await reclaimOpenCodeStorage();
      await onComplete(result);
      dialog.current?.close();
    } catch (cause) {
      setError(String(cause));
    } finally {
      setWorking(false);
    }
  };
  return <>
    <button class="ghost-button reclaim-button" disabled={disabled} onClick={() => { setError(null); dialog.current?.showModal(); }}>{t.reclaimStorage}</button>
    <dialog class="dialog reclaim-dialog" ref={dialog} aria-labelledby="reclaim-title"
      onKeyDown={(event) => event.stopPropagation()}
      onCancel={(event) => { if (working) event.preventDefault(); }}>
      <h2 id="reclaim-title">{t.reclaimStorage}</h2>
      <p>{t.reclaimDescription}</p>
      <p>{t.closeOpenCode}</p>
      {error && <p class="reclaim-error" role="alert">{error}</p>}
      <div class="dialog-actions">
        <button class="ghost-button" disabled={working} onClick={() => dialog.current?.close()}>{t.cancel}</button>
        <button class="danger-button" disabled={working} onClick={() => void reclaim()}>{working ? t.working : t.reclaimStorage}</button>
      </div>
    </dialog>
  </>;
}

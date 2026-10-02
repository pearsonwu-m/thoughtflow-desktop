# Privacy

Thoughts can be personal. Thoughtflow is built so that what you write stays on your Mac unless you explicitly ask Claude for something.

## What is stored, and where

Everything is local:

- **Thoughts, conversations, plans, tasks, and generated prompts:** a SQLite database at `~/Library/Application Support/com.thoughtflow.desktop/thoughtflow.db`.
- **Settings:** `settings.json` in the same folder. It contains no secrets.
- **Anthropic API key:** the macOS Keychain (service `com.thoughtflow.desktop`, account `anthropic-api-key`). Only the Rust backend reads it. The interface can ask whether a key exists and see a masked hint (`sk-ant-…abcd`), but never the key itself.

Thoughtflow has no server, no account, no analytics, and no telemetry.

## What is sent to Anthropic, and when

Text goes to the Anthropic API **only** when you do one of these:

| Action | What is sent |
| --- | --- |
| ⌘↵ (send), a mode shortcut with an empty composer, ⌘K (clarify), Regenerate | Thoughtflow's fixed instructions, **the current thought's conversation**, the mode you chose, today's date, and any related notes you left selected |
| Save plan / Save tasks | The current thought's conversation as plain text, today's date, and extraction instructions |
| Test connection | Nothing about your thoughts. It only lists the models your key can use |

While a request is in flight, the widget shows **"Sending to Claude · model"**.

**Related notes** are excerpts from your past thoughts. They're suggested only while you type a new thought (or in Reflect mode), and they're shown as chips under the composer before you send. Deselect any chip to keep that note out of the request. To keep a thought out of suggestions permanently, open it and choose **⋯ → Keep out of Claude's memory**. You can turn suggestions off entirely in **Settings → Memory**.

Thoughtflow **never** sends your other thoughts, files, clipboard, screen contents, window titles, or anything about which apps you use. It does not read background desktop information at all.

Anthropic's handling of API data is described in its [privacy policy](https://www.anthropic.com/legal/privacy) and [commercial terms](https://www.anthropic.com/legal/commercial-terms).

## Logs

Thoughtflow does not log thought content. Error messages shown in the app are generic sentences that never include what you wrote.

## Export and deletion

In **Settings → Memory**:

- **Export data…** writes everything you've saved to a readable JSON file you choose.
- **Clear history…** permanently deletes all thoughts, conversations, plans, tasks, and prompts.
- **Delete all data…** also resets settings and removes the API key from your Keychain.

Deleting a single thought (⋯ → Delete permanently, or ⌘⌫ in History) removes it together with its conversation, plans, tasks, and prompts. The database runs with `secure_delete` on, so deleted content is overwritten rather than left in free pages. The write-ahead log is truncated after deletions, and the file is vacuumed after clearing history.

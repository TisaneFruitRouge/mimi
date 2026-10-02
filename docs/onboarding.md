# Onboarding

The first run: a short welcome that sets up a model and, optionally, the user's accounts
and a few facts about them. It lives in `apps/desktop/src/features/onboarding/` and
replaces the old setup mode.

## Rules

- It's shown while `settings.onboarding_done` is false. The current step is saved in
  `settings.onboarding_step`, so closing the app midway resumes there; once finished it
  never shows again on its own.
- Settings › General › "Show the welcome again" reruns it.
- Migration 0010 marks existing setups as onboarded.
- A model chosen for download is saved as `settings.pending_model`; the daemon makes it
  the default when the download finishes (`settings::adopt_pending`), whichever download
  finishes it, even with no window open. So the user can keep going, or close the app,
  while it downloads.
- Onboarding may name models like the Models page does (elsewhere, no model names
  outside Models). The Models page in setup mode remains the fallback when a finished
  setup has lost its model.
- Cloud services are offered with the trade-off said plainly; on weak machines they're
  recommended first (hardware detection drives the recommendations).

## Steps

1. **Welcome**: what Mimi is and the privacy promise.
2. **Model**: a plain-language summary of this computer (hardware), models already on it,
   the recommended downloads for it (built-in runtime, else Ollama), local servers found
   running, or a cloud service (with an honest note on the trade-off; recommended first
   on weak machines). See [Models](models.md).
3. **Connections** (optional): calendar, email, and Telegram, Signal or Matrix, through
   the Connections dialogs. See [Connections](connections.md).
4. **About you** (optional): name, place and free text, appended to the memory profile.
   See [Memory](memory.md).
5. **Finish**: waits for the download if one is running; "Start chatting".

The welcome shows the assistant's character (happy, then idle); see
[Design system](design-system.md). Where onboarding sits in the app's navigation is in
[Frontend](frontend.md).

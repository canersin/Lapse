# UI Translations

To add a new language or customize an existing one:

1. Copy `tr.toml` to `<code>.toml` (for example `de.toml`, `fr.toml`, `es.toml`).
2. Set `[meta] name` to the native name of the language (for example `name = "Deutsch"`).
3. Translate the right side of each `"English" = "translation"` pair under `[strings]`.
4. Any missing lines stay in English (automatic fallback).
5. Write `\"` for a quote inside strings.
6. Put the file in `lang/` in the repository or in `<config dir>/lang/` to try it without rebuilding (e.g. `~/.config/lapse/lang/` on Linux). Files in the config directory override built-in ones with the same code.

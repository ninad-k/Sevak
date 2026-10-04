# AI assistant

Ask a question in the launcher and read the answer without leaving it. Type `ai ` and a question, press ++enter++, and the answer appears as a result you can copy, paste or read in full.

!!! warning "Off by default, and your questions leave this computer"
    The assistant is **off until you turn it on** (**Settings → AI assistant**). With **Ollama** the question stays on your computer. With **OpenAI-compatible** or **Anthropic**, the question is sent over the internet to that service when you press ++enter++, and the service's own privacy terms apply to it. [Exactly what is sent](#what-is-sent-and-to-whom) is listed below.

## Set it up

1. Open **Settings → AI assistant**.
2. Choose a **provider** and switch **Use the AI assistant** on.
3. For OpenAI or Anthropic, paste an **API key** and press **Save key**. For Ollama there is no key.
4. Press **Test connection** to check the address, the key and the model, then **Save**.

| Provider | Talks to | Key | Default model |
|---|---|---|---|
| **Ollama** (the default) | `http://localhost:11434`, a model running on your computer | none | `llama3.2` |
| **OpenAI-compatible** | `https://api.openai.com/v1` (ChatGPT / OpenAI), or the **base URL** you set | `OPENAI_API_KEY` or a saved key | `gpt-4o-mini` |
| **Anthropic** | `https://api.anthropic.com` | `ANTHROPIC_API_KEY` or a saved key | `claude-haiku-4-5` |

The default models are only a starting point: providers retire models, so type the name you want into **Model**.

**Ollama** needs the [Ollama](https://ollama.com) app running and the model downloaded once (`ollama pull llama3.2`). **Test connection** says if the model is missing.

**OpenAI-compatible** works with any server that copies OpenAI's chat API: set the **Base URL**, for example `http://localhost:1234/v1` for LM Studio, `https://openrouter.ai/api/v1` for OpenRouter or `https://api.groq.com/openai/v1` for Groq. A server on your own computer needs no key. Azure OpenAI uses a different sign-in header and is not supported.

## Using it

| You type | What you see | ++enter++ |
|---|---|---|
| `ai ` | a hint saying where a question will go | nothing |
| `ai why is the sky blue` | **Ask "why is the sky blue"**, with the model and host it will be sent to | sends the question |
| (while it waits) | **Asking gpt-4o-mini…** | nothing; the action panel (++ctrl+k++) has **Cancel the question** |
| (the answer) | the answer, then **Open full answer** for a long one | copies the answer |
| (a failure) | a row that says what went wrong | tries again |

On the answer: ++shift+enter++ pastes it into the app you were using (where pasting works), ++ctrl+t++ or **Open full answer** reads it all in the text view, and the action panel (++ctrl+k++) has **Ask again**. The launcher stays open while the question is on its way, and the answer replaces the **Asking…** row.

Typing never sends anything: only ++enter++ on the **Ask** row does. There is no streaming; the answer appears when it is complete. Each question is separate: the assistant does not remember earlier ones.

### Ask AI about selection

Select text in any app and press the [Universal Actions](features/selection.md) key. **Ask AI about selection** puts `ai Explain this: <your selection>` in the search box so you can edit it. **Nothing is sent until you press ++enter++** there. It is offered only for text, only when the assistant is on, and a selection longer than 2,000 characters is cut at that length.

## What is sent, and to whom

When you press ++enter++ on an **Ask** row, Sevak makes **one HTTPS request** to the **base URL's host** containing:

- your question (or the text you chose with **Ask AI about selection**, after you pressed ++enter++),
- the **system prompt** from Settings,
- the **model name** and the **maximum answer length**,
- your **API key**, in the header the provider expects (never in the address or the body).

Nothing else goes with it: not your clipboard, your files, your search history, the apps you use, or an identifier. The only extra header is `User-Agent: Sevak/<version> (ai)`.

| Provider | Host that receives it | Request |
|---|---|---|
| Ollama | `localhost:11434` (or your base URL) | `POST /api/chat` |
| OpenAI-compatible | `api.openai.com` (or your base URL) | `POST /chat/completions` |
| Anthropic | `api.anthropic.com` (or your base URL) | `POST /v1/messages` |

**Test connection** makes a different request: `GET` of the provider's model list (`/models`, `/v1/models` or `/api/tags`), with the key if there is one. It sends **no question** and runs only when you press the button.

Sevak makes no AI request in any other situation: not at startup, not while you type, not when you open the launcher, and not in the background. There is no telemetry, no retry loop and no caching of what you ask. See [Privacy](privacy.md#ai-assistant).

### What is kept

The question and the answer live **in memory** until you ask something else, reload the settings or quit Sevak. They are **not** written to disk, **not** logged, and **not** added to the search history or usage statistics. (The log records only that a question was asked, the provider and how long it took.)

What the provider keeps is up to the provider: see the terms of OpenAI, Anthropic or the service behind your base URL.

## API keys

- A key is **never** written to `config.toml`, a settings export or the [diagnostics report](privacy.md#diagnostics-report), and never to the log.
- Keys you save in Settings are kept in `ai-keys.json` in the [data folder](files-and-data.md):
    - on **Windows**, encrypted with **DPAPI** for your user account (only your Windows account on this computer can read it back);
    - on **macOS and Linux**, in a file only your user account can read (mode `0600`). It is **not encrypted** there, like the [clipboard history](features/clipboard.md); Sevak does not use the macOS Keychain or the Linux Secret Service.
- If nothing is saved, the `OPENAI_API_KEY` or `ANTHROPIC_API_KEY` **environment variable** is used. A key saved in Settings takes precedence.
- The key field is **write-only**: once saved, it is never shown again. **Remove** deletes it.
- A key is **never sent over plain `http://` to another computer**: Sevak refuses and tells you to use `https://`. Redirects are never followed, so a key cannot be passed on to another address.

## Safety

- **The answer is untrusted text.** It is shown and copied as plain text. Nothing in it is run, opened or interpreted, and Sevak never creates a result that launches, opens or executes anything from it. Control characters and text-direction overrides are removed first.
- **Everything is capped.** Questions: 8,000 characters. The system prompt: 4,000 characters. A request: 64 KiB. A reply from the server: 1 MiB (a larger one is discarded). An answer: 20,000 characters. The wait: the **Timeout** setting (5 to 300 seconds).
- **Offline is fine.** Without a connection, or with Ollama not running, you get a clear message, and the rest of Sevak is unaffected.
- **Questions you cancel** stop being shown at once. The request itself runs until its timeout, but its reply is thrown away.

## Troubleshooting

| Message | What to do |
|---|---|
| **Ollama is not running** | Start the Ollama app (or `ollama serve`), or fix the base URL. |
| **That model was not found** | For Ollama run `ollama pull <model>`; otherwise check the model name. |
| **No API key for …** | Paste a key under **Settings → AI assistant**, or set `OPENAI_API_KEY` / `ANTHROPIC_API_KEY` and restart Sevak. |
| **The AI service did not accept the API key** | The key is wrong, revoked or lacks access. Save it again. |
| **The AI service is busy or over its limit** | Wait, or check your credit and rate limits with the provider. |
| **The AI service took too long** | Raise **Timeout**; a local model may still be loading. |
| **The answer was too large** | Lower **Longest answer**; the server sent more than 1 MiB. |
| **The model used its whole token budget before answering** | Raise **Longest answer**. This happens with reasoning models. |
| **The stored API key could not be read** | It was saved by another Windows user or computer. Enter it again. |
| **The AI assistant is off** | Switch it on in **Settings → AI assistant** (and make sure **AI assistant** is on under **Plugins**). |

## Options

All of these are in **Settings → AI assistant**; the [`[ai]` section](configuration.md#ai) of `config.toml` has the same keys. The API key is the one thing that is not in the file.

| Setting | Default | What it does |
|---|---|---|
| Use the AI assistant | off | Nothing is sent unless this is on |
| Keyword | `ai` | What you type before the question |
| Provider | Ollama | `ollama`, `openai` or `anthropic` |
| Model | provider's default | Which model answers |
| Base URL | provider's default | Where requests go |
| System prompt | a short "be concise" instruction | Sent with every question |
| Longest answer | 512 tokens | 16 to 8192 |
| Timeout | 60 seconds | 5 to 300 |

Turn the plugin off completely by adding `"ai"` to [`[plugins] disabled`](configuration.md#plugins).

## Not included

Streaming answers, conversation memory, images and files, tool use, and Azure OpenAI's sign-in are not supported. The assistant answers one question at a time and never acts on your computer.

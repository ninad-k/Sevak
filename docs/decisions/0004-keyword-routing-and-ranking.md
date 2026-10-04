# ADR-0004: Keyword and global routing, and how results are ranked

**Status:** Accepted

## Context

One input box has to serve very different intents: launch an app, do a sum,
search the web, find a file. Running every plugin for every keystroke gives
noisy results and wastes time; making the user pick a mode first is slow.

## Decision

The engine (`crates/sevak-core/src/engine.rs`, described at the top of the file
and in [How Sevak works](../architecture.md#keyword-routing-and-ranking))
routes in three steps:

1. **Keyword route.** `<keyword> <text>` goes only to the plugin or plugins
   that own the keyword, with no fallback. Symbol keywords such as `>` need no
   space.
2. **Global route.** Otherwise every plugin that answers un-keyworded input is
   queried. Results from a plugin that has its own keyword but also answers
   globally (files) are down-weighted so dedicated sources (apps) stay on top.
3. **Fallback.** If nothing answered, the configured fallback (web search by
   default) receives the whole input.

Ranking is a fuzzy score from `nucleo-matcher`, fixed bands for answers that
are not fuzzy matches (`EXACT_ANSWER`, `KEYWORD`, `FALLBACK` in `model.rs`),
and a usage boost that halves every 72 hours. The boost can reorder comparable
matches but cannot lift a weak match over a strong one. Ties break by title
and then id, so the order is deterministic.

A keyword that two plugins answer is allowed, not rejected; Sevak logs it once
(`KeywordOwners::log_shared`) and the workflow builder reports clashes.

## Consequences

- Predictable: the same input and the same usage give the same order.
- Per-keystroke cost stays bounded because a keyword route touches few
  plugins; a query slower than 16 ms is logged with the slowest plugin.
- Learning from usage means `usage.json` is a small behavioural record.
  Plugins whose results contain private text opt out (`Plugin::tracks_usage`),
  and the search history is a setting.
- A script plugin or workflow can claim a keyword that a built-in plugin also
  answers. Both then answer; nothing is silently replaced.

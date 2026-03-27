# Changelog

All notable changes to this project will be documented in this file.

## [Unreleased]

### Fixed
- **Memory Tools**: Removed the `memory_get_summary` tool and the underlying `summaries` table.
  - *Bugfix Context*: AI agents generating and storing summaries from raw data proved to be counterproductive, as the quality of summaries degrades over time and varies between different models (e.g., Claude vs. Gemini). Relying on the `memory_get_summary` tool occasionally caused agents to hallucinate or get confused when an empty summary was returned.
  - *Resolution*: Agents will now dynamically generate context directly from the raw fast-search engine (FTS) entries, ensuring higher accuracy and eliminating state sync issues.

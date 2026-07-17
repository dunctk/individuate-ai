# Memory Timeline Feature Plan

Status: Planned

## Purpose

Give users a calm, chronological view of meaningful life events without turning every remembered conversation into a timeline entry.

The timeline is a curated projection of existing episodic memory. It does not replace `episodes`, session history, the mind map, relationship profiles, or vector recall.

## Product Principles

- Store concrete episodes generously, but display timeline landmarks selectively.
- Treat episodes as factual records and timeline prominence as editable presentation metadata.
- Prefer the user's words and explicit choices over model-inferred importance.
- Merge new details into an existing event instead of creating near-duplicates.
- Preserve uncertainty: show approximate dates rather than inventing precision.
- Keep every timeline decision reversible through pin, hide, edit, and merge controls.

## What Qualifies as a Key Event

An episode can appear as a key event when it is concrete and has at least one strong significance signal or several supporting signals.

Strong signals:

- The user explicitly pins the event or asks for it to be remembered as important.
- The event begins or ends a meaningful life chapter.
- It creates a lasting change in circumstances or a close relationship.

Supporting signals:

- The event is revisited across separate sessions.
- It becomes linked to multiple people, goals, needs, or mind-map concepts.
- Later conversations materially update or correct it.
- It records a clear decision, turning point, or consequence.

Routine daily updates and abstract patterns remain available to memory recall but do not automatically become timeline cards.

## Saturation Controls

### Separate storage from display

All qualifying concrete episodes may remain stored and searchable. Only promoted episodes appear on the default timeline.

### Merge related developments

Later details about the same event update the existing episode. Closely related episodes may be grouped beneath a parent event, so a breakup and its later practical consequences can occupy one timeline position with expandable developments.

Automated merging should propose a match using semantic similarity, shared participants, date proximity, and linked concepts. It must not rely only on an LLM-generated episode ID.

### Compress by age and density

- Show recent key events individually.
- Group dense periods into expandable month or chapter sections.
- Reduce older periods to landmarks and user-pinned events by default.
- Limit the collapsed view to roughly three to five cards per month before grouping.

No event is deleted by this compression; it only changes the default presentation.

## Timeline Card

Each card should show:

- Date or honest date range, including labels such as `Around March 2026`.
- A short, neutral title grounded in the user's account.
- A concise factual narrative.
- Linked people and mind-map concepts.
- Whether the event was revisited or expanded later.

User quotes, source sessions, and individual developments remain behind an expanded view to avoid visual and emotional overload.

Available actions:

- Pin or unpin.
- Hide from timeline without forgetting.
- Edit the title, date, or narrative.
- Merge duplicates or separate incorrectly merged events.
- Mark an event as more or less significant.
- Ask why the event appears on the timeline.

## Proposed Data Model

Keep `episodes` as the source of truth and add encrypted presentation metadata in a separate table:

```sql
CREATE TABLE episode_timeline_metadata (
    user_id TEXT NOT NULL,
    episode_id TEXT NOT NULL,
    visibility TEXT NOT NULL DEFAULT 'normal',
    pinned INTEGER NOT NULL DEFAULT 0,
    date_precision TEXT NOT NULL DEFAULT 'unknown',
    parent_episode_id TEXT,
    significance_signals_ciphertext BLOB,
    last_revisited_at TEXT,
    created_at TEXT DEFAULT CURRENT_TIMESTAMP,
    updated_at TEXT DEFAULT CURRENT_TIMESTAMP,
    PRIMARY KEY (user_id, episode_id),
    FOREIGN KEY(user_id, episode_id) REFERENCES episodes(user_id, id)
        ON DELETE CASCADE
);
```

Suggested values:

- `visibility`: `normal`, `landmark`, or `hidden`.
- `date_precision`: `day`, `month`, `season`, `year`, or `unknown`.

Any human-authored metadata and significance explanations must follow the same per-user encryption requirements as other private memory content.

## Ranking and Promotion

Promotion should be rules-based and explainable. Explicit user actions override automated ranking.

A first version can promote an episode when:

1. It is pinned or explicitly identified as a landmark; or
2. It contains a lasting-change or chapter-boundary signal; or
3. It accumulates multiple supporting signals, such as repeated recall plus several memory links.

Do not use emotional intensity alone. A painful event is not necessarily something the user wants made prominent.

## API Shape

Potential authenticated endpoints:

- `GET /api/timeline` returns grouped timeline cards and date precision.
- `PATCH /api/timeline/:episode_id` updates pin, visibility, date, or display text.
- `POST /api/timeline/merge` merges or groups selected episodes.
- `POST /api/timeline/:episode_id/separate` reverses a grouping.

All operations must use the authenticated user scope; no client- or model-controlled `user_id` should determine access.

## Mobile Experience

Design first for the 375 x 667 CSS-pixel iPhone SE viewport.

- Use a single vertical line with compact cards rather than a two-sided desktop timeline.
- Keep touch targets at least 44 x 44 CSS pixels.
- Collapse details and quotes by default.
- Preserve safe-area padding and usable controls when the on-screen keyboard is open.
- Use restrained motion and honor reduced-motion preferences.

## Delivery Stages

1. Add encrypted timeline metadata, date precision, repository methods, and tests.
2. Add deterministic significance signals, duplicate matching, grouping, and a read API.
3. Build the mobile-first timeline screen with expansion, pin, and hide controls.
4. Add edit, merge, separate, and `Why is this here?` interactions.
5. Verify encryption migration, user isolation, accessibility, mobile layouts, and duplicate handling.

## Acceptance Criteria

- The default timeline contains meaningful events rather than every stored episode or session.
- New details about an existing event enrich or group with it instead of producing obvious duplicates.
- Approximate dates are displayed without false precision.
- Users can pin, hide, edit, merge, and reverse a merge.
- Hiding an event does not remove it from private memory recall.
- The UI explains promotion using understandable signals without exposing private internal identifiers.
- Timeline data remains encrypted at rest and scoped to the authenticated user.
- The collapsed mobile view stays usable during dense periods and meets minimum touch-target requirements.

## Non-Goals for the First Version

- Rendering every chat session chronologically.
- Automatically diagnosing life phases or assigning clinical meaning to events.
- Deleting source memories when an event is hidden.
- Sharing or exporting the timeline.
- Inferring exact dates that the user did not provide.

---
name: Torro Calendar
description: Use when the person asks about their schedule or wants an event created, moved, changed or deleted.
---
# Torro Calendar

## Steps

- Look: calendar_list_events with `start` and `end` around the days asked about. For "next" or
  "upcoming", start now and look a week ahead before you say there is nothing.
- Create: calendar_list_calendars to find a calendar that can be written to (ask which one when there
  are several and the person did not say), then calendar_save_event without `href`.
- Change: calendar_list_events first, then calendar_save_event with the whole event as you read it
  (`href`, `etag` and every field), only your changes made.
- Delete: calendar_delete_event with the event as calendar_list_events returned it.
- A place: calendar_search_places for the event's `location` and `geo`.
- From a mail: calendar_create_event_from_mail, or mail_extract_action for a suggestion to accept.

## Data

- Times are RFC 3339 with the person's time zone offset (`2026-11-17T14:30:00+01:00`). All-day events
  use dates. Say times in the person's time zone with the weekday.
- A series (`recurring: true`) needs `scope` for every change and deletion: `occurrence` (only this
  one), `following` (this and all after it) or `series` (all). Ask the person which they mean when it
  is not clear.
- Calendar ids are `<account-id>/<calendar URL>`; pass them on as they are.

## Typical errors

- `conflict`: the event changed meanwhile. Read it again with calendar_list_events, then save.
- `unsupported` on a series without `scope`: add the scope the person wants.
- A calendar that cannot be written to: pick another one or ask.

## Approvals

- Deleting events, and changes that drop events of a series, need full access and may wait for the
  person's approval. The line of calendars under "This conversation" says what waits for you.
- Before you delete or move many events, say what you will do and ask.

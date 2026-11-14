# Inbox

The **Notification Hub** gathers what comes in: mail, Nextcloud notifications, Talk, GitHub, ntfy, AI agents that wait or are done, and approvals. One entry per thread, with a counter. The inbox app has two more windows, "Mail" and "App Feed".

## Lanes

There are two lanes, "For me" and "Info". Only "For me" counts on the tray icon and may pop up. An item is "Done" or comes back "Later".

How items are sorted:

- Fixed rules first: direct mail from people you know, mentions and review requests go to "For me"; mailing lists, bulk mail and bots go to "Info".
- The first mail from a new sender asks once: For me, Info or Block.
- Moving an item to the other lane asks whether that holds always for this sender.
- AI suggestions for the lane are optional and always marked as suggestions.

Urgent items always get through: approvals, an agent that waits, a server that is down, a failing check on your own pull request, and senders you marked as important.

Entries close themselves when their cause is gone, for example a mail read elsewhere or a merged pull request.

## Muting

The bell on an item mutes things like it: for 1 hour, until tomorrow or always, with "No popup", "Only Info" or "Hide". "All rules" leads to every rule. Mail is never hidden this way; use a mail filter instead (see the topic "mail"). Approvals and entries of the suite itself cannot be muted.

"Done", "Later" and muting can be undone for 5 seconds with `u` or Ctrl + Z.

## Settings

The Notification Hub settings have "Notifications" (with "Quiet during busy events": no popups or sounds during busy events in your calendar except urgent items, and one summary afterwards), "Sources" and "Rules", where each rule can be removed.

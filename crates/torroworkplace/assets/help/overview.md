# Overview

Torro Workplace is a suite of small desktop apps around one core. The core runs once per session and shows a tray icon; it keeps your workspace templates, accounts, rights and approvals. The apps never store anything themselves: they ask the core.

## The apps

- **Notification Hub, Mail and App Feed** (the inbox app): everything that comes in. See the topics "inbox" and "mail".
- **Calendar**: your events from CalDAV calendars, for example Nextcloud.
- **Tasks**: to-dos from CalDAV task lists and GitHub issues.
- **Contacts**: your address books (CardDAV).
- **Chat**: Nextcloud Talk with a contact list and one window per conversation.
- **Assistant**: an AI chat that can work in the suite with the rights you give it.

Apps whose part you switched off are hidden. Contacts has no entry in the tray menu; open it from the app launcher.

## The tray menu

A click on the tray icon opens its menu, from top to bottom:

- while voice is on: what the microphone does, and "Mute microphone";
- the apps: "Notification Hub" (with the number of waiting items), "Mail", "App Feed", "Calendar", "Tasks", "Assistant", "Chat";
- "Pause reminders" with "For 1 hour" or "Until tomorrow" (then "Resume reminders");
- your workspace templates, "Save workspace…" and "Manage workspaces…";
- with a chat account: your own status ("Online", "Away", "Busy", "Do not disturb", "Invisible");
- "Settings…" and "Quit".

## Settings

"Settings…" opens the settings window **Torro Workplace**. Its sidebar: "Overview", "Apps", "Accounts", "AI", "API clients", "MCP clients", "MCP servers", "Approvals", "Workspaces", "Appearance", "Privacy", and "General" at the foot. Each app also has its own settings behind its gear; at their bottom, "Appearance" and "All settings" lead back to Torro Workplace.

## Three ways to every function

What you can do in the interface, programs can do through the API and AI agents through MCP, with the rights you grant them. See the topic "agents-and-mcp".

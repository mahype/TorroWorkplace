# Agents and MCP

Everything you can do in the interface, programs can do through the **API** and AI agents through **MCP**, with the rights you give each of them. Agents such as Claude Code, Codex, Gemini CLI, Claude Desktop and Cursor connect over MCP.

## Connect an agent

Torro Workplace › "MCP clients" › "Connect client…":

1. Choose the agent. Agents the suite found offer a "Guided setup", which writes the connection into the agent's own settings. For any other there is "Other MCP client": it shows the key once to copy; it is never shown again.
2. Give it a "Name" and a starting point for its rights: "Read only", "Read and write" or "Full access".
3. "Connect", then "Check connection" or "Finish without test".

"Connected clients" lists them, each with "Disconnect…". In its group "Notification Hub", "Report when waiting or done" lets the agent say in the Notification Hub when it waits for you or is done.

Scripts and other programs use the API with a key from Torro Workplace › "API clients" › "Create key…" ("Copy the key now."); "Revoke…" ends a key.

## Rights

Rights are set per area: no access, read, read and write, or full; narrower per command and for chosen calendars, lists, repositories, inbox sources and mail accounts. What a client may not see behaves as if it were not there. Each app's gear › "API and MCP" shows the rights of all keys and agents for that app.

"Approve before" sets when a change waits for you: never, on delete (the default) or on every change. Sending mail out has its own setting: never, with approval (the default) or free.

## Approvals

A request that waits for you shows as a notification with "Allow" and "Deny", as a number on the tray icon, in the Notification Hub and in Torro Workplace › "Approvals". It ends after 10 minutes. Only you decide: an agent can never approve its own request or change its own rights.

## MCP servers

Torro Workplace › "MCP servers" adds other MCP servers ("Add server…") and holds the "Built-in tools". A server is closed to every agent until you open it for that agent by name.

## Secrets

Passwords, tokens and keys of your accounts never leave the core, on no way and to no agent.

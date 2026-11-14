# Accounts

Accounts connect the suite to your servers. Add them in Torro Workplace › "Accounts": one group per kind, each with its button, or "Add account", which asks "Choose a service to connect." first.

## Nextcloud

"Add Nextcloud account…": enter the address of your Nextcloud, then sign in with Nextcloud in your browser. If the browser does not open, the window shows a link to copy. A Nextcloud account brings calendars, task lists, contacts, notifications and chat.

## Mail

"Add mail account…": "Email address", "Password" and "Sender name". The suite looks up the mail provider from the part after the @. Under "Advanced" you can set "Incoming mail (IMAP)" and "Outgoing mail (SMTP)" yourself. The group "Calendars and tasks (CalDAV)" adds calendars and tasks of the same provider; leave it empty to use the account for mail only. "Import from TorroMail" takes over accounts from TorroMail.

You can add as many mail accounts as you like: each one is its own "Add mail account…". They show together in the Mail window and each has its own settings there.

## GitHub

"Add GitHub account…": "Use GitHub CLI sign-in" if the GitHub CLI is signed in, or "Paste a token" ("Create a token on GitHub"). For private repositories the token needs the scope `repo`. GitHub brings issues and pull requests as tasks and notifications.

## AI providers

"Add AI account…": choose where your models run, "On this computer" or "Cloud", then the provider, an API key where one is needed, and a model ("Suggested models", "Prepare model").

- On this computer: Ollama, LM Studio, llama.cpp, vLLM.
- Cloud: OpenRouter for open models, and Anthropic, OpenAI, Mistral.

Models that run outside this computer are off after connecting; only you can turn them on, in Torro Workplace › "AI".

## ntfy

"Add ntfy account…" receives push messages from an ntfy server in the Notification Hub.

## Secrets

Passwords, tokens and API keys stay with the core. No app, program or AI agent ever gets them back, not even the assistant; the interface only says whether one is stored.

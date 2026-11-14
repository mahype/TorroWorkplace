# Workspaces

Torro Workplace saves a workspace of your tiling window manager as a **template** and brings it back later: the windows, their split layout and how to start each app.

Supported window managers: **Hyprland** on Linux (for example Omarchy), **AeroSpace** on macOS and **GlazeWM** on Windows. Without one of them, templates do not work; the rest of the suite does.

## Save and load

- Tray menu › "Save workspace…" saves the active workspace at once under an automatic name and opens a dialog to name it. "Cancel" in that dialog removes the template it just saved.
- Each template in the tray menu has "Load", "Replace windows" and "Edit…". "Load" adds its windows to the workspace; "Replace windows" replaces what is there.
- With no templates the menu says "No templates yet".
- Tray menu › "Manage workspaces…" or Torro Workplace › "Workspaces" lists them, each with "Load", "Edit…", "Update from current workspace" and "Delete…".

## The template dialog

"Save workspace" or "Edit template" holds:

- "Name";
- a key, one digit 1 to 0;
- "Load at startup", with "Screen" and "If the screen is missing" ("Use the main screen" or "Skip").

A template loaded at startup is loaded once per login on the screen you chose.

## Keys

A template with a key loads with a key combination: the modifier plus its digit. The modifiers are Super + Alt on Linux, Ctrl + Option on macOS and Ctrl + Shift on Windows by default; change them under Torro Workplace › "Workspaces" › "Shortcuts". A digit belongs to one template only: giving it to another takes it away from the first. Torro Workplace registers the keys while it runs and does not change the configuration of your window manager. When a key cannot be registered, "Shortcuts" says why.

## Good to know

- Templates belong to one computer; they are not shared between systems.

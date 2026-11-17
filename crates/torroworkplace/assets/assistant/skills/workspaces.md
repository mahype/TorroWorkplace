---
name: Torro Workspaces
description: Use when the person wants a workspace template saved, loaded, updated, renamed or deleted, or asks what templates there are.
---
# Torro Workspaces

## Steps

- Look: workspaces_list_templates (id, name, number of windows, last change).
- Save: workspaces_save_template saves the windows of the active workspace as a new template. Ask for a
  name when the person did not give one, or let the core pick one.
- Load: workspaces_load_template opens the template's apps on the active workspace and moves their
  windows. Always say what will open and ask first. Mode `empty` loads only into an empty workspace,
  `add` keeps the windows there, `replace` closes them first.
- Update: workspaces_update_template replaces the template with the windows of the active workspace.
- Rename, key, startup: workspaces_rename_template, workspaces_set_template_shortcut,
  workspaces_set_template_startup (with a screen from workspaces_list_screens).
- Delete: workspaces_delete_template.

## Data

- Templates are found by their id from workspaces_list_templates; match the person's words to a name and
  ask when several fit.
- Everything works on the active workspace: the one the person sees now.

## Typical errors

- `workspace-not-empty` with mode `empty`: windows are open there. Ask whether to add (`add`), close them
  (`replace`) or switch to an empty workspace first.
- `busy`: another template is being saved or loaded; try again in a moment.
- `unsupported`: the window manager is not one the suite supports (see "This computer"). `error`: it
  may not be running; say what failed.

## Approvals

- `replace` closes windows and needs delete access; update and delete lose the old template for good.
  They may wait for the person's approval. The line "Waits for the
  person's approval" under "This conversation" says what waits for you.
- Before you close windows or overwrite a template, say so and ask, even when you may do it.

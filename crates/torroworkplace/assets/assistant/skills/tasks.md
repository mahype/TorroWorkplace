---
name: Torro Tasks
description: Use when the person asks about their tasks or wants one created, done, changed, planned, focused or deleted.
---
# Torro Tasks

## Steps

- Look: tasks_list_todos; tasks_list_lists for the lists and which can be written to.
- Create: tasks_save_todo without `href` in a list that can be written to (ask which one when there are
  several and the person did not say).
- Change or mark done: tasks_list_todos first, then tasks_save_todo with the whole task as you read it
  (`href`, `etag` and every field), only your changes made.
- Plan for a day: tasks_plan_todo with `day` (YYYY-MM-DD). Order in a list: tasks_move_todo.
- Focus: tasks_set_focus with `normal`, `focus` or `strong`.
- Delete: tasks_delete_todo with the task as tasks_list_todos returned it.
- From a mail: tasks_create_todo_from_mail; from an inbox entry: inbox_create_task_from_item.

## Data

- A field left out of tasks_save_todo is cleared: `url`, `categories`, `sort_order`, `link` and `focus`
  above all. Always send them back as you read them.
- GitHub filter lists (`github:<login>/<list-id>`) take no new or deleted tasks; marking one of their
  issues done closes it on GitHub, so say so before you do it.
- Due dates in the person's time zone; say them with the weekday.

## Typical errors

- `conflict`: the task changed meanwhile; read it again, then save. On focus: today's focus slots are
  all taken. Ask which task gives its focus up (`release`), never pick one yourself.
- `unsupported` when moving a task to a list of another account: create it there instead, if the person
  wants that.
- `forbidden` on a GitHub link: there is no read access to GitHub.

## Approvals

- Deleting a task may wait for the person's approval. The line "Waits for the
  person's approval" under "This conversation" says what waits for you.
- Before you change or delete many tasks, say what you will do and ask.

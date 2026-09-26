-- Workspace Setups shortcuts, loaded from ~/.config/hypr/bindings.lua.
--   SUPER + F1..F10          load the workspace template assigned to that key
--   SUPER + SHIFT + F1..F10  save the active workspace as that key's template
-- Both go through the Omarchy shell so the plugin can ask before replacing
-- open windows or overwriting a template.
for slot = 1, 10 do
  o.bind("SUPER + F" .. slot, "Load workspace template F" .. slot,
    "omarchy-shell -q workspace-setups loadSlot " .. slot)
  o.bind("SUPER + SHIFT + F" .. slot, "Save workspace as template F" .. slot,
    "omarchy-shell -q workspace-setups saveSlot " .. slot)
end

#!/bin/sh
# A stand-in for the MCP server of an ArtCraft app (issue #303), for the process tests of the folders and the
# checks of the paths. It is no MCP implementation and no ArtCraft: it answers each JSON-RPC request line with a
# fixed line and needs nothing but sh. Saved as the catalog server `wordcraft` whose spec points at this script.
#
# It reports WordCraft's tool names (the blocked ones among them) and one more:
#   args   answers the arguments it was started with (the bounds Torro appended)
# Every call, of any tool, is written as its request line to ./calls.log in the folder it runs in, before it is
# answered; a tool other than `args` answers "done <tool>".

started="$*"

answer() {
    printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$1" "$2"
}

text() {
    answer "$1" "{\"content\":[{\"type\":\"text\",\"text\":\"$2\"}],\"isError\":false}"
}

tool() {
    printf '{"name":"%s","description":"%s","inputSchema":{"type":"object"}}' "$1" "$2"
}

tools="$(tool list_commands 'Lists commands'),$(tool get_text 'Reads the text'),$(tool open_document 'Opens a file')"
tools="$tools,$(tool save_document 'Saves'),$(tool execute 'Runs a command'),$(tool batch 'Runs commands')"
tools="$tools,$(tool screenshot 'Takes a screenshot'),$(tool click 'Clicks'),$(tool key 'Presses a key')"
tools="$tools,$(tool ui_inspect 'Inspects the window'),$(tool args 'Its arguments')"

while IFS= read -r line; do
    id=$(printf '%s\n' "$line" | sed -n 's/^{"jsonrpc":"2.0","id":\([0-9][0-9]*\),.*/\1/p')
    # Notifications have no id and get no answer.
    [ -n "$id" ] || continue
    case "$line" in
        *'"method":"initialize"'*)
            answer "$id" '{"protocolVersion":"2025-06-18","capabilities":{"tools":{}},"serverInfo":{"name":"artcraft-fixture","version":"0.11.0"}}'
            ;;
        *'"method":"tools/list"'*)
            answer "$id" "{\"tools\":[$tools]}"
            ;;
        *'"method":"tools/call"'*)
            printf '%s\n' "$line" >> calls.log
            name=$(printf '%s\n' "$line" | sed -n 's/.*"name":"\([A-Za-z_]*\)".*/\1/p')
            case "$name" in
                args) text "$id" "$started" ;;
                *) text "$id" "done $name" ;;
            esac
            ;;
        *'"method":"ping"'*)
            answer "$id" '{}'
            ;;
        *)
            printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32601,"message":"method not found"}}\n' "$id"
            ;;
    esac
done

#!/bin/sh
# A stand-in for an MCP server over stdio, for the process tests of the supervisor (task MA.4). It is no MCP
# implementation: it answers each JSON-RPC request line with a fixed line and needs nothing but sh, sed and sleep.
#
# Arguments, before it reads anything:
#   grandchild   starts `sleep` in the background (its own child) and writes its pid to ./grandchild.pid
#   stderr N     writes N numbered lines to stderr
# Tools:
#   echo   answers "pong" and the value of $FIXTURE_VALUE
#   pwd    answers the folder it runs in
#   slow   answers after 30 seconds
#   crash  ends the program with status 3

while [ "$#" -gt 0 ]; do
    case "$1" in
        grandchild)
            # Without stdin, stdout and stderr: the core must not wait for it to close them.
            sleep 300 </dev/null >/dev/null 2>&1 &
            echo "$!" > grandchild.pid
            ;;
        stderr)
            shift
            n=1
            while [ "$n" -le "$1" ]; do
                echo "line $n of the server's output" >&2
                n=$((n + 1))
            done
            ;;
    esac
    shift
done

answer() {
    printf '{"jsonrpc":"2.0","id":%s,"result":%s}\n' "$1" "$2"
}

text() {
    answer "$1" "{\"content\":[{\"type\":\"text\",\"text\":\"$2\"}],\"isError\":false}"
}

while IFS= read -r line; do
    id=$(printf '%s\n' "$line" | sed -n 's/^{"jsonrpc":"2.0","id":\([0-9][0-9]*\),.*/\1/p')
    # Notifications have no id and get no answer.
    [ -n "$id" ] || continue
    case "$line" in
        *'"method":"initialize"'*)
            answer "$id" '{"protocolVersion":"2025-06-18","capabilities":{"tools":{"listChanged":true}},"serverInfo":{"name":"fixture","version":"1.0.0"}}'
            ;;
        *'"method":"tools/list"'*)
            answer "$id" '{"tools":[{"name":"echo","description":"Answers pong","inputSchema":{"type":"object"}},{"name":"pwd","description":"Where it runs","inputSchema":{"type":"object"}},{"name":"slow","description":"Takes long","inputSchema":{"type":"object"}},{"name":"crash","description":"Ends","inputSchema":{"type":"object"}}]}'
            ;;
        *'"method":"tools/call"'*)
            case "$line" in
                *'"name":"echo"'*) text "$id" "pong $FIXTURE_VALUE" ;;
                *'"name":"pwd"'*) text "$id" "$(pwd)" ;;
                *'"name":"slow"'*) sleep 30; text "$id" "late" ;;
                *'"name":"crash"'*) exit 3 ;;
                *) printf '{"jsonrpc":"2.0","id":%s,"error":{"code":-32602,"message":"no such tool"}}\n' "$id" ;;
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

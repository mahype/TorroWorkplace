#!/usr/bin/env bash
#
# worktree.sh — Git-Worktrees je Issue fuer den Issue-Workflow (docs/issue-workflow.md)
#
# Uebernommen aus enon (scripts/worktree.sh), ohne DDEV, Datenbank und Editor:
# Torro braucht fuer einen Worktree nur Git.
#
# Usage:
#   ./scripts/worktree.sh create <issue-nr> [--yes] [--allow-stale-base]   Worktree mit Issue-Branch anlegen
#   ./scripts/worktree.sh list                                             Alle Issue-Worktrees anzeigen
#   ./scripts/worktree.sh prune [<issue-nr>...] [--yes]                    Worktrees mit gemergtem PR und ohne eigene Aenderungen aufraeumen
#   ./scripts/worktree.sh agent <issue-nr|worktree-pfad> <name> <prompt> [--model <modell>] [--exit-caller]
#                                                                          In Herdr einen Tab im Worktree oeffnen, Claude starten, Prompt schicken
#

set -euo pipefail

# ── Config ──────────────────────────────────────────────────────────────────
# Das Hauptprojekt nicht aus dem Skriptpfad ableiten: Aus einem Worktree heraus
# zeigte $(dirname $0)/.. auf genau diesen Worktree. Der erste Eintrag von
# `git worktree list` ist immer der Haupt-Checkout. sed statt `head -1`, damit
# git unter pipefail kein SIGPIPE bekommt.
SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
MAIN_PROJECT_DIR="$(git -C "$SCRIPT_DIR" worktree list --porcelain 2>/dev/null | sed -n '1s/^worktree //p' || true)"
if [[ -z "$MAIN_PROJECT_DIR" || ! -d "$MAIN_PROJECT_DIR" ]]; then
    echo "FEHLER: Hauptprojekt-Verzeichnis konnte nicht bestimmt werden." >&2
    exit 1
fi
REPO="mahype/TorroWorkplace"
MAIN_PROJECT_NAME="torro"
BASE_BRANCH="main"
# Wird von fetch_base_ref() gesetzt: normalerweise "origin/main", nach einem
# bewusst durchgewinkten Fetch-Fehler der lokale Ref.
BASE_REF=""
WORKTREES_DIR="${MAIN_PROJECT_DIR}/.worktrees"

# Persoenliche Einstellungen (gitignored, nur im Haupt-Checkout): je Zeile
# KEY=wert ohne Anfuehrungszeichen. Wird nur gelesen, nicht ausgefuehrt. Eine
# gesetzte Umgebungsvariable hat Vorrang.
LOCAL_CONFIG="${MAIN_PROJECT_DIR}/scripts/worktree.local.env"
if [[ -f "$LOCAL_CONFIG" ]]; then
    while IFS='=' read -r key value || [[ -n "$key" ]]; do
        [[ "$key" =~ ^[A-Z_][A-Z0-9_]*$ ]] || continue
        [[ -n "${!key+x}" ]] && continue
        printf -v "$key" '%s' "${value%$'\r'}"
    done < "$LOCAL_CONFIG"
fi

# ── Herdr ────────────────────────────────────────────────────────────────────
# Fokus: Der Umsetzungs-Tab (impl-<nr>) kommt nach vorn, der Plan-Tab
# (plan-<nr>) bleibt im Hintergrund, damit sich mehrere Plaene hintereinander
# starten lassen. Uebergabe: Planung an, Umsetzung aus.
HERDR_FOCUS_IMPL="${HERDR_FOCUS_IMPL:-true}"
HERDR_FOCUS_PLAN="${HERDR_FOCUS_PLAN:-false}"
HERDR_HANDOFF_PLAN="${HERDR_HANDOFF_PLAN:-true}"
HERDR_HANDOFF_IMPL="${HERDR_HANDOFF_IMPL:-false}"

# ── Colors ──────────────────────────────────────────────────────────────────
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
BOLD='\033[1m'
NC='\033[0m'

# Zwei Leerzeichen nach ℹ und ⚠: Viele Terminals zeichnen sie doppelt breit.
info()    { echo -e "${BLUE}ℹ${NC}  $*"; }
success() { echo -e "${GREEN}✓${NC} $*"; }
warn()    { echo -e "${YELLOW}⚠${NC}  $*"; }
error()   { echo -e "${RED}✗${NC} $*" >&2; }

slugify() {
    echo "$1" \
        | sed 's/[Ää]/ae/g; s/[Öö]/oe/g; s/[Üü]/ue/g; s/[ß]/ss/g' \
        | tr '[:upper:]' '[:lower:]' \
        | sed 's/[^a-z0-9]/-/g' \
        | sed 's/--*/-/g' \
        | sed 's/^-//; s/-$//'
}

# ── Titel → Branch-Slug (hoechstens drei Woerter) ───────────────────────────
# Funktionswoerter tragen im Branch-Namen keinen Inhalt. Die Liste steht in der
# Form nach slugify (fuer, ueber). Bleibt nach dem Filtern nichts uebrig, gilt
# der ungefilterte Slug.
SLUG_STOPWORDS=" als mit und oder fuer von vom zu zum zur der die das den dem des ein eine einen einem einer im in an am auf aus bei beim nach ohne ueber unter statt nicht kein keine ist sind wird werden bzw auch the a and or for of to on with "

branch_slug() {
    local full word
    local filtered=()
    full=$(slugify "$1")
    local IFS='-'
    for word in $full; do
        [[ "$SLUG_STOPWORDS" == *" ${word} "* ]] || filtered+=("$word")
    done
    if (( ${#filtered[@]} == 0 )); then
        echo "$full" | cut -d'-' -f1-3
        return
    fi
    echo "${filtered[*]:0:3}"
}

# Pfad des Worktrees zu einer Issue-Nummer, leer wenn keiner existiert.
find_worktree_by_issue() {
    local issue_nr="$1" wt_path dir_name
    while IFS= read -r wt_path; do
        dir_name=$(basename "$wt_path")
        if [[ "$dir_name" =~ ^${MAIN_PROJECT_NAME}-${issue_nr}(-|$) ]]; then
            echo "$wt_path"
            return
        fi
    done < <(git -C "$MAIN_PROJECT_DIR" worktree list --porcelain | sed -n 's/^worktree //p')
}

# Basis-Ref aktualisieren, bevor etwas angelegt wird: Sonst landet ein neuer
# Worktree still auf einem veralteten lokalen Stand.
fetch_base_ref() {
    local allow_stale="${1:-false}" auto_yes="${2:-false}" fetch_err=""

    info "Hole aktuellen Stand von origin..."
    if fetch_err=$(git -C "$MAIN_PROJECT_DIR" fetch --quiet origin 2>&1); then
        if ! git -C "$MAIN_PROJECT_DIR" show-ref --verify --quiet "refs/remotes/origin/${BASE_BRANCH}"; then
            error "Remote-Branch 'origin/${BASE_BRANCH}' existiert nicht."
            exit 1
        fi
        BASE_REF="origin/${BASE_BRANCH}"
        success "Basis: ${BASE_REF} ($(git -C "$MAIN_PROJECT_DIR" log -1 --format='%h %s' "$BASE_REF"))"
        return 0
    fi

    error "'git fetch origin' fehlgeschlagen — kein Netz oder kein Zugang zu origin."
    if [[ -n "$fetch_err" ]]; then
        echo "$fetch_err" | sed 's/^/  git: /'
    fi
    if [[ "$allow_stale" == true ]]; then
        BASE_REF="$BASE_BRANCH"
        warn "--allow-stale-base gesetzt — weiter auf dem lokalen '${BASE_BRANCH}'."
        return 0
    fi
    if [[ "$auto_yes" == true ]]; then
        echo "  Abbruch. Netz/Zugang pruefen, oder bewusst mit --allow-stale-base anlegen."
        exit 1
    fi
    local stale_input=""
    read -rp "Trotzdem auf dem lokalen Stand fortfahren? [y/N] " stale_input || stale_input=""
    if [[ "$stale_input" == "y" || "$stale_input" == "Y" ]]; then
        BASE_REF="$BASE_BRANCH"
        warn "Weiter auf dem lokalen '${BASE_BRANCH}' — der Stand kann veraltet sein."
        return 0
    fi
    error "Abgebrochen."
    exit 1
}

# Branch-Praefix aus den Typ-Labels (docs/issue-workflow.md, "Issue-Typen").
# Das Repo gehoert einem Nutzer, GitHub-Issue-Typen gibt es dort nicht.
prefix_from_labels() {
    local labels=",$1,"
    case "$labels" in
        *,bug,*) echo "bug" ;;
        *,feature,*|*,enhancement,*) echo "feature" ;;
        *,task,*) echo "task" ;;
        *,update,*) echo "update" ;;
        *,docs,*|*,documentation,*) echo "docs" ;;
        *) echo "" ;;
    esac
}

# ── Command: create ─────────────────────────────────────────────────────────
cmd_create() {
    local issue_nr="${1:-}" auto_yes=false allow_stale_base=false
    shift || true
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --yes|-y) auto_yes=true ;;
            --allow-stale-base) allow_stale_base=true ;;
            # Aus enon uebernommene Schalter, hier ohne Wirkung.
            --no-ddev|--no-editor) ;;
            *) error "Unbekanntes Argument: $1"; exit 1 ;;
        esac
        shift
    done
    if [[ ! "$issue_nr" =~ ^[0-9]+$ ]]; then
        error "Usage: $0 create <issue-nr> [--yes] [--allow-stale-base]"
        exit 1
    fi
    command -v gh >/dev/null 2>&1 || { error "gh CLI nicht installiert"; exit 1; }
    command -v jq >/dev/null 2>&1 || { error "jq nicht installiert"; exit 1; }

    fetch_base_ref "$allow_stale_base" "$auto_yes"

    info "Hole Issue #${issue_nr} von GitHub..."
    local issue_json issue_title labels prefix
    issue_json=$(gh issue view "$issue_nr" --repo "$REPO" --json title,labels 2>/dev/null) || {
        error "Issue #${issue_nr} nicht gefunden"
        exit 1
    }
    issue_title=$(jq -r '.title' <<<"$issue_json")
    labels=$(jq -r '[.labels[].name] | join(",")' <<<"$issue_json")
    # printf statt echo -e: Backslashes im Titel blieben sonst nicht erhalten.
    printf "${BLUE}ℹ${NC}  Issue #%s: ${BOLD}%s${NC}\n" "$issue_nr" "$issue_title"

    prefix=$(prefix_from_labels "$labels")
    if [[ -n "$prefix" ]]; then
        info "Typ-Label erkannt → Präfix: ${prefix}/"
    elif [[ "$auto_yes" == true ]]; then
        prefix="feature"
        warn "Kein Typ-Label gefunden, verwende Default: feature/"
    else
        echo ""
        echo -e "${YELLOW}Kein Typ-Label gefunden. Welcher Typ?${NC}"
        echo "  1) feature  2) bug  3) task  4) docs  5) update"
        local type_choice=""
        read -rp "Auswahl [1]: " type_choice || type_choice=""
        case "${type_choice:-1}" in
            2) prefix="bug" ;;
            3) prefix="task" ;;
            4) prefix="docs" ;;
            5) prefix="update" ;;
            *) prefix="feature" ;;
        esac
    fi

    local slug branch_name
    slug=$(branch_slug "$issue_title")
    branch_name="${prefix}/${issue_nr}-${slug}"

    # Gibt es schon genau einen Branch zum Issue (lokal oder auf origin), diesen
    # nehmen, sonst entstuende ein zweiter Branch neben der bisherigen Arbeit.
    local existing_branches
    existing_branches=$(git -C "$MAIN_PROJECT_DIR" for-each-ref --format='%(refname)' \
        "refs/heads/*/${issue_nr}-*" "refs/remotes/origin/*/${issue_nr}-*" \
        | sed 's#^refs/heads/##; s#^refs/remotes/origin/##' | sort -u || true)
    if [[ -n "$existing_branches" ]]; then
        if [[ $(printf '%s\n' "$existing_branches" | wc -l) -eq 1 ]]; then
            branch_name="$existing_branches"
            info "Vorhandener Branch zum Issue gefunden: ${BOLD}${branch_name}${NC}"
        else
            warn "Mehrere Branches zu Issue #${issue_nr} gefunden, keiner wird automatisch genommen:"
            printf '    %s\n' $existing_branches
        fi
    fi

    info "Branch: ${BOLD}${branch_name}${NC}"
    if [[ "$auto_yes" != true ]]; then
        local branch_input=""
        read -rp "Übernehmen? [Y/n] oder neuen Namen eingeben: " branch_input || branch_input=""
        if [[ -n "$branch_input" && "$branch_input" != "y" && "$branch_input" != "Y" ]]; then
            branch_name="$branch_input"
        fi
    fi

    local dir_slug="$slug"
    if [[ "$branch_name" =~ ^[a-z]+/${issue_nr}-(.+)$ ]]; then
        dir_slug=$(slugify "${BASH_REMATCH[1]}")
    fi
    local worktree_path="${WORKTREES_DIR}/${MAIN_PROJECT_NAME}-${issue_nr}-${dir_slug}"

    local existing_worktree
    existing_worktree=$(find_worktree_by_issue "$issue_nr")
    if [[ -n "$existing_worktree" ]]; then
        error "Für Issue #${issue_nr} gibt es bereits einen Worktree: ${existing_worktree}"
        exit 1
    fi
    if [[ -d "$worktree_path" ]]; then
        error "Verzeichnis existiert bereits: ${worktree_path}"
        exit 1
    fi
    if git -C "$MAIN_PROJECT_DIR" worktree list | grep -qF "[${branch_name}]"; then
        error "Branch '${branch_name}' ist bereits in einem Worktree ausgecheckt"
        exit 1
    fi

    mkdir -p "$WORKTREES_DIR"
    if git -C "$MAIN_PROJECT_DIR" show-ref --verify --quiet "refs/heads/${branch_name}"; then
        git -C "$MAIN_PROJECT_DIR" worktree add "$worktree_path" "$branch_name"
    elif git -C "$MAIN_PROJECT_DIR" show-ref --verify --quiet "refs/remotes/origin/${branch_name}"; then
        git -C "$MAIN_PROJECT_DIR" worktree add "$worktree_path" "$branch_name"
    else
        git -C "$MAIN_PROJECT_DIR" worktree add --no-track -b "$branch_name" "$worktree_path" "$BASE_REF"
    fi
    success "Worktree erstellt: ${worktree_path}"

    # In Herdr als Workspace oeffnen, damit seine Chats nicht unter dem
    # Haupt-Checkout landen. Nicht fatal.
    if [[ "${HERDR_ENV:-}" == "1" ]] && command -v herdr >/dev/null 2>&1; then
        if herdr worktree open --cwd "$MAIN_PROJECT_DIR" --path "$worktree_path" --no-focus >/dev/null; then
            success "Worktree in Herdr geöffnet"
        else
            warn "Herdr-Workspace konnte nicht geöffnet werden: herdr worktree open --path ${worktree_path}"
        fi
    fi

    echo ""
    printf '  Issue:       #%s — %s\n' "$issue_nr" "$issue_title"
    echo "  Branch:      ${branch_name}"
    echo "  Verzeichnis: ${worktree_path}"
}

# ── Command: list ───────────────────────────────────────────────────────────
cmd_list() {
    echo ""
    echo -e "${BOLD}Issue-Worktrees:${NC}"
    echo ""
    printf "  ${BOLD}%-45s %s${NC}\n" "VERZEICHNIS" "BRANCH"
    local line wt_path wt_branch found=false
    while IFS= read -r line; do
        wt_path=$(awk '{print $1}' <<<"$line")
        wt_branch=$(sed 's/.*\[//; s/\]//' <<<"$line")
        [[ "$(basename "$wt_path")" == ${MAIN_PROJECT_NAME}-* ]] || continue
        printf "  %-45s %s\n" "$(basename "$wt_path")" "$wt_branch"
        found=true
    done < <(git -C "$MAIN_PROJECT_DIR" worktree list)
    [[ "$found" == true ]] || echo "  (keine)"
    echo ""
    echo "  Ort: ${WORKTREES_DIR}"
    echo ""
}

# ── Command: prune ──────────────────────────────────────────────────────────
# Rueckgabe 0 = darf weg, 1 = bleibt (Grund auf stdout), 2 = gh fehlgeschlagen.
prune_check() {
    local p="$1" b="$2" oids oid merged=false changes

    [[ -d "$p" ]] || { echo "Ordner fehlt, mit 'git worktree prune' aufräumen"; return 1; }
    if ! oids=$(gh pr list --repo "$REPO" --head "$b" --base "$BASE_BRANCH" \
            --state merged --json headRefOid --jq '.[].headRefOid' </dev/null); then
        echo "gh pr list fehlgeschlagen"
        return 2
    fi
    [[ -n "$oids" ]] || { echo "kein gemergter PR für '${b}' nach ${BASE_BRANCH}"; return 1; }

    while IFS= read -r oid; do
        [[ -n "$oid" ]] || continue
        if git -C "$p" merge-base --is-ancestor HEAD "$oid" 2>/dev/null; then
            merged=true
            break
        fi
    done <<< "$oids"
    [[ "$merged" == true ]] || { echo "Commits nach dem gemergten PR-Stand (oder PR-Stand lokal unbekannt)"; return 1; }

    changes=$(git -C "$p" status --porcelain --untracked-files=all 2>/dev/null) || { echo "git status fehlgeschlagen"; return 1; }
    if [[ -n "$changes" ]]; then
        echo "Änderungen: $(printf '%s\n' "$changes" | sed 's/^...//' | sed -n '1,5p' | paste -sd ',' - | sed 's/,/, /g')"
        return 1
    fi
    return 0
}

cmd_prune() {
    local -a wanted=()
    local auto_yes=false nr
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --yes|-y) auto_yes=true ;;
            *) [[ "$1" =~ ^[0-9]+$ ]] || { error "Usage: $0 prune [<issue-nr> ...] [--yes]"; exit 1; }
               wanted+=("$1") ;;
        esac
        shift
    done
    command -v gh >/dev/null 2>&1 || { error "gh fehlt — ohne PR-Status wird nichts entfernt."; exit 1; }

    # Kandidaten aus `git worktree list --porcelain`: Bloecke durch Leerzeilen
    # getrennt, erster Block ist der Haupt-Checkout. Ohne branch-Zeile
    # (detached) gibt es keinen PR, der Eintrag faellt weg.
    local -a cand_nr=() cand_pfad=() cand_branch=() skip_text=()
    local line wt_path="" wt_branch="" first=true
    while IFS= read -r line; do
        if [[ "$line" == worktree\ * ]]; then wt_path="${line#worktree }"; wt_branch=""; continue; fi
        if [[ "$line" == branch\ refs/heads/* ]]; then wt_branch="${line#branch refs/heads/}"; continue; fi
        [[ -n "$line" ]] && continue
        if [[ "$first" == true ]]; then
            first=false
        elif [[ -n "$wt_path" && "$wt_branch" =~ ^[a-z]+/([0-9]+)- ]]; then
            nr="${BASH_REMATCH[1]}"
            if [[ ${#wanted[@]} -eq 0 || " ${wanted[*]} " == *" ${nr} "* ]]; then
                cand_nr+=("$nr"); cand_pfad+=("$wt_path"); cand_branch+=("$wt_branch")
            fi
        fi
        wt_path=""; wt_branch=""
    done < <(git -C "$MAIN_PROJECT_DIR" worktree list --porcelain; echo)

    local -a weg_nr=() weg_pfad=() weg_branch=()
    local i reason rc
    for i in "${!cand_nr[@]}"; do
        rc=0
        reason=$(prune_check "${cand_pfad[$i]}" "${cand_branch[$i]}") || rc=$?
        if (( rc == 2 )); then
            error "gh pr list für '${cand_branch[$i]}' fehlgeschlagen — prune bricht ohne Löschen ab."
            exit 1
        fi
        if (( rc != 0 )); then
            skip_text+=("#${cand_nr[$i]} $(basename "${cand_pfad[$i]}") — ${reason}")
            continue
        fi
        weg_nr+=("${cand_nr[$i]}"); weg_pfad+=("${cand_pfad[$i]}"); weg_branch+=("${cand_branch[$i]}")
    done

    if [[ ${#skip_text[@]} -gt 0 ]]; then
        info "Übersprungen:"
        printf "  %s\n" "${skip_text[@]}"
    fi
    if [[ ${#weg_nr[@]} -eq 0 ]]; then
        info "Nichts zu entfernen."
        return 0
    fi
    warn "Wird entfernt (PR gemergt, keine eigenen Änderungen):"
    for i in "${!weg_nr[@]}"; do printf "  #%-6s %s\n" "${weg_nr[$i]}" "${weg_pfad[$i]}"; done
    if [[ "$auto_yes" != true ]]; then
        local confirm=""
        read -rp "Entfernen? [y/N]: " confirm || confirm=""
        [[ "$confirm" == "y" || "$confirm" == "Y" ]] || { info "Abgebrochen."; return 0; }
    fi
    for i in "${!weg_nr[@]}"; do
        git -C "$MAIN_PROJECT_DIR" worktree remove "${weg_pfad[$i]}" \
            && git -C "$MAIN_PROJECT_DIR" branch -D "${weg_branch[$i]}" >/dev/null \
            && success "#${weg_nr[$i]} entfernt" \
            || warn "#${weg_nr[$i]} konnte nicht entfernt werden"
    done
}

# ── Command: agent ──────────────────────────────────────────────────────────
cmd_agent() {
    local issue_nr="" name="" prompt="" model="" exit_caller=false
    while [[ $# -gt 0 ]]; do
        case "$1" in
            --model)
                [[ $# -ge 2 ]] || { error "--model braucht einen Wert"; exit 1; }
                model="$2"; shift 2 ;;
            --exit-caller) exit_caller=true; shift ;;
            *)
                if [[ -z "$issue_nr" ]]; then issue_nr="$1"
                elif [[ -z "$name" ]]; then name="$1"
                elif [[ -z "$prompt" ]]; then prompt="$1"
                else error "Unbekanntes Argument: $1"; exit 1
                fi
                shift ;;
        esac
    done
    if [[ -z "$issue_nr" || -z "$name" || -z "$prompt" ]]; then
        error "Usage: $0 agent <issue-nr|worktree-pfad> <name> <prompt> [--model <modell>] [--exit-caller]"
        exit 1
    fi
    if [[ ! "$name" =~ ^[a-z][a-z0-9_-]{0,31}$ ]]; then
        error "Ungültiger Agent-Name '${name}' (erlaubt: [a-z][a-z0-9_-]{0,31})"
        exit 1
    fi
    if [[ "${HERDR_ENV:-}" != "1" ]] || ! command -v herdr >/dev/null 2>&1; then
        warn "Nicht in Herdr — kein Tab geöffnet. Sitzung im Worktree selbst starten."
        exit 3
    fi
    local handoff=true
    case "$name" in
        plan-*) handoff="$HERDR_HANDOFF_PLAN" ;;
        impl-*) handoff="$HERDR_HANDOFF_IMPL" ;;
    esac
    if [[ "$handoff" != "true" ]]; then
        warn "Übergabe an einen neuen Tab abgeschaltet — kein Tab geöffnet (HERDR_HANDOFF_PLAN/HERDR_HANDOFF_IMPL in scripts/worktree.local.env)."
        exit 4
    fi
    command -v jq >/dev/null 2>&1 || { error "jq nicht gefunden"; exit 1; }

    local worktree_path
    if [[ "$issue_nr" =~ ^[0-9]+$ ]]; then
        worktree_path=$(find_worktree_by_issue "$issue_nr")
        [[ -n "$worktree_path" ]] || { error "Kein Worktree für Issue #${issue_nr} gefunden"; exit 1; }
    else
        # Worktree ohne Issue-Nummer (quick-fix): Pfad statt Nummer.
        worktree_path="$issue_nr"
        if [[ ! -d "$worktree_path" ]] || \
           ! git -C "$MAIN_PROJECT_DIR" worktree list --porcelain | grep -Fqx "worktree ${worktree_path}"; then
            error "Kein Worktree unter '${worktree_path}' gefunden"
            exit 1
        fi
    fi

    # Ein untaetig wartender Agent gleichen Namens bekommt nur den Prompt; einer
    # mitten im Turn nichts, sonst landete der Prompt in einem fremden Turn.
    local existing="" status=""
    if existing=$(herdr agent get "$name" 2>/dev/null); then
        status=$(jq -r '.result.agent.agent_status // empty' <<<"$existing" 2>/dev/null || true)
        if [[ "$status" != "idle" && "$status" != "done" ]]; then
            herdr agent focus "$name" >/dev/null 2>&1 || true
            error "Agent '${name}' läuft schon in Herdr (Zustand: ${status:-unbekannt}) — Tab fokussiert, keinen Prompt geschickt."
            exit 1
        fi
        info "Agent '${name}' wartet schon (Zustand: ${status}) — kein neuer Tab, nur der Prompt."
    else
        local workspace_id
        workspace_id=$(herdr_workspace_for "$worktree_path")
        if [[ -z "$workspace_id" ]]; then
            herdr worktree open --cwd "$MAIN_PROJECT_DIR" --path "$worktree_path" --no-focus >/dev/null
            workspace_id=$(herdr_workspace_for "$worktree_path")
        fi
        [[ -n "$workspace_id" ]] || { error "Kein Herdr-Workspace für ${worktree_path}"; exit 1; }

        local pane_id
        pane_id=$(herdr tab create --workspace "$workspace_id" --cwd "$worktree_path" --label "$name" --no-focus \
            | jq -r '.result.root_pane.pane_id')

        local -a agent_args=()
        [[ -n "$model" ]] && agent_args=(-- --model "$model")
        # Eine frische Pane ist einige Sekunden noch keine Shell; herdr meldet
        # dann agent_pane_busy. Nur diesen Fehler wiederholen.
        local attempt=1 start_out
        until start_out=$(herdr agent start "$name" --kind claude --pane "$pane_id" --timeout 120000 ${agent_args[@]+"${agent_args[@]}"} 2>&1); do
            if [[ "$start_out" != *agent_pane_busy* || $attempt -ge 10 ]]; then
                printf '%s\n' "$start_out" >&2
                error "Claude ließ sich in Pane ${pane_id} nicht starten."
                exit 1
            fi
            attempt=$((attempt + 1))
            sleep 1
        done
    fi

    if [[ "$exit_caller" == true ]]; then
        local prompt_out
        if prompt_out=$(herdr agent prompt "$name" "$prompt" --wait --until working --until blocked --timeout 30000 2>&1); then
            schedule_caller_exit "$issue_nr" "$name"
        elif [[ "$prompt_out" == *agent_prompt_stalled* || "$prompt_out" == *timeout* ]]; then
            warn "Prompt an '${name}' geschickt, aber kein Arbeitsbeginn erkannt — diese Sitzung bleibt offen."
        else
            printf '%s\n' "$prompt_out" >&2
            error "Prompt an '${name}' fehlgeschlagen — diese Sitzung bleibt offen."
            exit 1
        fi
    else
        herdr agent prompt "$name" "$prompt" >/dev/null
    fi

    if [[ ( "$name" == impl-* && "$HERDR_FOCUS_IMPL" == "false" ) || ( "$name" == plan-* && "$HERDR_FOCUS_PLAN" == "false" ) ]]; then
        success "Herdr-Tab '${name}' im Hintergrund geöffnet: ${prompt}"
        return 0
    fi
    herdr agent focus "$name" >/dev/null || true
    success "Herdr-Tab '${name}' geöffnet: ${prompt}"
}

# Beendet die aufrufende Claude-Sitzung abgekoppelt, damit dieser Aufruf sofort
# zurueckkehrt. Alle drei Deskriptoren umgelenkt, sonst wartet das Bash-Tool auf
# die Subshell.
schedule_caller_exit() {
    local issue_nr="$1" name="$2" caller="${HERDR_PANE_ID:-}" kind=""
    if [[ -z "$caller" ]]; then
        warn "HERDR_PANE_ID nicht gesetzt — diese Sitzung bleibt offen."
        return 0
    fi
    kind=$(herdr agent get "$caller" 2>/dev/null | jq -r '.result.agent.agent // empty' 2>/dev/null || true)
    if [[ "$kind" != "claude" ]]; then
        warn "In Pane ${caller} läuft keine Claude-Sitzung — nichts zu beenden."
        return 0
    fi
    ( exit_caller_when_idle "$issue_nr" "$name" "$caller" ) </dev/null >/dev/null 2>&1 &
    info "Diese Sitzung beendet sich nach ihrer Antwort selbst; es geht im Tab '${name}' weiter."
}

# Wartet, bis der laufende Turn der aufrufenden Sitzung vorbei ist, und schickt
# erst dann /exit. Fragt sie etwas (blocked) oder endet der Turn nicht binnen
# 10 Minuten, bleibt sie offen.
exit_caller_when_idle() {
    local issue_nr="$1" name="$2" caller="$3" waited status agents remaining i
    waited=$(herdr agent wait "$caller" --until idle --until done --until blocked --timeout 600000) || return 0
    status=$(jq -r '.result.agent.agent_status // empty' <<<"$waited" 2>/dev/null || true)
    [[ "$status" == "idle" || "$status" == "done" ]] || return 0

    herdr notification show "Issue #${issue_nr}: Sitzung beendet" --body "Es geht im Tab ${name} weiter." || true
    herdr agent prompt "$caller" "/exit" || return 0
    for ((i = 0; i < 30; i++)); do
        remaining=""
        if agents=$(herdr agent list 2>/dev/null); then
            remaining=$(jq -r --arg p "$caller" '[.result.agents[]? | select(.pane_id == $p)] | length' <<<"$agents" 2>/dev/null || true)
        fi
        if [[ "$remaining" == "0" ]]; then
            herdr pane run "$caller" "echo 'Sitzung fuer #${issue_nr} beendet: Es geht im Herdr-Tab ${name} weiter.'" || true
            return 0
        fi
        sleep 1
    done
}

# Herdr-Workspace-ID des Worktrees, leer wenn er in Herdr nicht offen ist.
herdr_workspace_for() {
    local wanted="$1"
    herdr worktree list --cwd "$MAIN_PROJECT_DIR" \
        | jq -r --arg p "$wanted" \
            '.result.worktrees[] | select((.path | ascii_downcase) == ($p | ascii_downcase)) | .open_workspace_id // empty'
}

# ── Main ────────────────────────────────────────────────────────────────────
main() {
    local command="${1:-}"
    shift || true
    case "$command" in
        create) cmd_create "$@" ;;
        list)   cmd_list "$@" ;;
        prune)  cmd_prune "$@" ;;
        agent)  cmd_agent "$@" ;;
        *)
            sed -n '3,14p' "$0" | sed 's/^# \{0,1\}//'
            exit 1
            ;;
    esac
}

main "$@"

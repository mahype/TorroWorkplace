#!/usr/bin/env bash
# Reproduces the screenshots of the settings audit (#968): every settings page of the control centre and of the
# apps, drawn in memory from the demo data (no window, no network, nothing written to the real home).
#
# usage: docs/ux/settings-audit/capture.sh [<area> ...]      areas: cc calendar tasks hub mail assistant
#
# Needs the debug binaries (CARGO_BUILD_JOBS=4 cargo build -p torroworkplace -p torrocalendar -p torrotasks
# -p torroinbox -p torroassistant). Writes screenshots/<area>/<page>_<look>_<size>.png, look = dark|light,
# size = wide (1600x1000) | medium (800x600) | narrow (480x900). Images are drawn at 2x like a Retina screen.
set -uo pipefail

root=$(cd "$(dirname "$0")/../../.." && pwd)
bin=${TORRO_BIN:-$root/target/debug}
out=$root/docs/ux/settings-audit/screenshots
scratch=${TMPDIR:-/tmp}/settings-audit-shots
log=${TMPDIR:-/tmp}/settings-audit-capture.log
: > "$log"

declare -a SIZES=("wide:1600x1000" "narrow:480x900")

# shot <area> <slug> <binary> <looks> <sizes> -- <args...>
# looks: "dark" or "dark light"; sizes: names from SIZES plus "medium"
shot() {
  local area=$1 slug=$2 prog=$3 looks=$4 sizes=$5
  shift 6
  local args=("$@")
  mkdir -p "$out/$area"
  for look in $looks; do
    for size in $sizes; do
      case $size in wide) geo=1600x1000 ;; medium) geo=800x600 ;; narrow) geo=480x900 ;; esac
      rm -rf "$scratch"; mkdir -p "$scratch"
      if [ "$prog" = torroworkplace ]; then sub=(ui); else sub=(); fi
      "$bin/$prog" ${sub[@]+"${sub[@]}"} --demo "${args[@]}" --appearance "$look" --shot-size "$geo" \
        --shots "$scratch" >/dev/null 2>>"$log"
      code=$?
      file=$(ls "$scratch"/*.png 2>/dev/null | head -1)
      if [ $code -eq 0 ] && [ -n "$file" ]; then
        mv "$file" "$out/$area/${slug}_${look}_${size}.png"
        echo "ok   $area/$slug $look $size" >> "$log"
      else
        echo "FAIL $area/$slug $look $size (exit $code): ${args[*]}" >> "$log"
        echo "FAIL $area/$slug $look $size (exit $code)"
      fi
    done
  done
}

want() { [ $# -eq 0 ] && return 0; local a; for a in "${ARGS[@]}"; do [ "$a" = "$AREA" ] && return 0; done; return 1; }
ARGS=("$@")
run_area() { AREA=$1; [ ${#ARGS[@]} -eq 0 ] && return 0; for a in "${ARGS[@]}"; do [ "$a" = "$1" ] && return 0; done; return 1; }

# ---- Control centre (torroworkplace ui) -------------------------------------------------------------------
if run_area cc; then
  for s in overview accounts ai api mcp connections approvals workspaces appearance privacy general; do
    shot cc "section-$s" torroworkplace "dark light" "wide narrow medium" -- --section "$s"
  done
  for s in menu drawer account-detail account-detail-ai mail-detail mail-detail-problem mail-password mail-servers \
           ntfy-detail ntfy-topic scopes-detail api-empty api-detail api-new-key api-revoke create-key create-key-done \
           mcp-empty mcp-detail mcp-hooks mcp-new-key mcp-disconnect mcp-bots mcp-orphans connect-client \
           connect-client-rights connect-client-done connect-client-other connect-opencode connect-open-webui \
           connections-builtin connections-own connections-empty search-providers catalog-install \
           catalog-install-tools extension-detail extension-review extension-crashed extension-folder \
           extension-unbounded extension-unusable custom-server custom-server-paste custom-server-choose \
           design design-schemes design-fonts design-switched approvals-empty workspaces-unsupported edit-template \
           delete-template terminal apps apps-menu apps-installing apps-managed apps-offline apps-error \
           accounts-empty accounts-ai remove change-password wizard-nextcloud wizard-ways wizard-token \
           wizard-mail wizard-mail-found wizard-mail-advanced wizard-mail-password wizard-mail-certificate \
           wizard-torromail wizard-ai wizard-ai-openrouter wizard-ai-server wizard-ai-cloud-form wizard-ntfy \
           wizard-ntfy-topics wizard-ntfy-login setup-welcome setup-mail setup-cloud setup-cloud-address \
           setup-ai setup-ai-chat setup-ai-sort setup-ai-keep setup-appearance setup-documents setup-done \
           setup-ai-form setup-ai-download; do
    shot cc "state-$s" torroworkplace "dark" "wide narrow" -- --open "$s"
  done
  shot cc "state-design_dense" torroworkplace dark "medium" -- --open design --design dense
  shot cc "state-design_comfort" torroworkplace dark "medium" -- --open design --design comfort
fi

# ---- Calendar -----------------------------------------------------------------------------------------------
if run_area calendar; then
  for s in calendars reminders night working-hours accounts api-mcp appearance privacy general; do
    shot calendar "settings-$s" torrocalendar "dark light" "wide narrow" -- --settings "$s"
  done
  shot calendar "settings-calendars" torrocalendar dark "medium" -- --settings calendars
fi

# ---- Tasks ----------------------------------------------------------------------------------------------------
if run_area tasks; then
  for s in lists accounts api-mcp appearance privacy general; do
    shot tasks "settings-$s" torrotasks "dark light" "wide narrow" -- --settings "$s"
  done
  for s in new-list new-task-list list-created filter-new filter-repos filter-edit filter-repo filter-search filter-delete more-list; do
    shot tasks "state-$s" torrotasks dark "wide narrow" -- --open "$s"
  done
fi

# ---- Notification Hub / App Feed (torroinbox, notifications view) ---------------------------------------
if run_area hub; then
  for s in notifications sources rules accounts api-mcp; do
    shot hub "settings-$s" torroinbox "dark light" "wide narrow" -- --view notifications --settings "$s"
  done
  shot hub "settings-notifications-hubview" torroinbox dark "wide" -- --view hub --settings notifications
  shot hub "settings-sources" torroinbox dark "medium" -- --view notifications --settings sources
  shot hub "state-sources-mail" torroinbox dark "wide narrow" -- --view notifications --open sources-mail
fi

# ---- Mail (torroinbox, mail view) -----------------------------------------------------------------------------
if run_area mail; then
  for s in accounts reading attachments categorization automation statistics ai api-mcp appearance privacy general; do
    shot mail "settings-$s" torroinbox "dark light" "wide narrow" -- --view mail --settings "$s"
  done
  for s in account account-folders account-writing account-storage account-sync account-question backup backup-plan \
           backup-error backup-done mail-accounts mail-accounts-many mail-accounts-selected categorization \
           categorization-category categorization-phishing categorization-categories categorization-sort \
           categorization-filter categorization-running categorization-network statistics statistics-full \
           statistics-empty statistics-loading statistics-error statistics-table statistics-senders \
           statistics-inbox statistics-categories automation workflow-setup workflow-template workflow-test \
           workflow-result workflow-error workflow-rule workflow-rule-preview workflow-documents; do
    shot mail "state-$s" torroinbox dark "wide narrow" -- --view mail --open "$s"
  done
fi

# ---- Assistant ------------------------------------------------------------------------------------------------
if run_area assistant; then
  for s in general voice skills bots access ai api-mcp suite-general; do
    shot assistant "settings-$s" torroassistant "dark light" "wide narrow" -- --settings "$s"
  done
  for s in profile-editor profile-changed access-selections voice-conversation access-bots bot-editor bot-new setup \
           no-engines; do
    shot assistant "state-$s" torroassistant dark "wide narrow" -- --open "$s"
  done
  shot assistant "settings-voice" torroassistant dark "medium" -- --settings voice
fi

echo "done, log: $log"

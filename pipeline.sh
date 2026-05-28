#!/usr/bin/env bash
# =============================================================================
# Memex Pipeline — трёхфазный запуск Claude Code с цветовой индикацией этапов
# =============================================================================
# Использование:
#   ./pipeline.sh phase1    # Архитектура (ADR + C4 + AGENTS.md)
#   ./pipeline.sh phase2    # План реализации
#   ./pipeline.sh phase3    # Subagent-ы + двухэтапный ревью
#
# Цветовая схема:
#   Phase 1 (Архитектура)  — Blue   🏗️
#   Phase 2 (Планирование) — Yellow 📋
#   Phase 3 (Имплементация)— Green  ⚡
#
# Каждый этап в отдельном окне терминала — визуально не перепутаешь.
# =============================================================================

set -euo pipefail

# ─── Цвета для баннеров ─────────────────────────────────────────────────────
BOLD='\033[1m'
RESET='\033[0m'

# Phase colours (background + foreground)
C1_BG='\033[48;5;33m'
C1_FG='\033[38;5;15m'
C2_BG='\033[48;5;220m'
C2_FG='\033[38;5;16m'
C3_BG='\033[48;5;40m'
C3_FG='\033[38;5;15m'

DIM='\033[2m'
CYAN='\033[36m'
MAGENTA='\033[35m'
YELLOW='\033[33m'
GREEN='\033[32m'
BLUE='\033[34m'

# ─── Баннеры ─────────────────────────────────────────────────────────────────
banner_phase1() {
    echo -e ""
    echo -e "${C1_BG}${C1_FG}                                                                  ${RESET}"
    echo -e "${C1_BG}${C1_FG}  █████╗ ██████╗  ██████╗██╗  ██╗██╗████████╗███████╗ ██████╗████████╗██╗   ██╗██████╗ ███████╗${RESET}"
    echo -e "${C1_BG}${C1_FG} ██╔══██╗██╔══██╗██╔════╝██║  ██║██║╚══██╔══╝██╔════╝██╔════╝╚══██╔══╝██║   ██║██╔══██╗██╔════╝${RESET}"
    echo -e "${C1_BG}${C1_FG} ███████║██████╔╝██║     ███████║██║   ██║   █████╗  ██         ██║   ██║   ██║██████╔╝█████╗  ${RESET}"
    echo -e "${C1_BG}${C1_FG} ██╔══██║██╔══██╗██║     ██╔══██║██║   ██║   ██╔══╝  ██         ██║   ██║   ██║██╔══██╗██╔══╝  ${RESET}"
    echo -e "${C1_BG}${C1_FG} ██║  ██║██║  ██║╚██████╗██║  ██║██║   ██║   ███████╗╚██████╗   ██║   ╚██████╔╝██║  ██║███████╗${RESET}"
    echo -e "${C1_BG}${C1_FG} ╚═╝  ╚═╝╚═╝  ╚═╝ ╚═════╝╚═╝  ╚═╝╚═╝   ╚═╝   ╚══════╝ ╚═════╝   ╚═╝    ╚═════╝ ╚═╝  ╚═╝╚══════╝${RESET}"
    echo -e "${C1_BG}${C1_FG}                                                                  ${RESET}"
    echo -e ""
    echo -e "${BLUE}${BOLD}Phase 1: Architecture Documentation${RESET}"
    echo -e "${DIM}ADR decisions → C4 diagrams → AGENTS.md${RESET}"
    echo -e "${DIM}Colour: ${BLUE}Blue${RESET} | ${DIM}Session: ${BLUE}🏗️ Architecture${RESET}"
    echo -e ""
}

banner_phase2() {
    echo -e ""
    echo -e "${C2_BG}${C2_FG}                                                                  ${RESET}"
    echo -e "${C2_BG}${C2_FG}  ██████╗ ██╗      █████╗ ███╗   ██╗███╗   ██╗██╗███╗   ██╗ ██████╗ ${RESET}"
    echo -e "${C2_BG}${C2_FG}  ██╔══██╗██║     ██╔══██╗████╗  ██║████╗  ██║██║████╗  ██║██╔════╝ ${RESET}"
    echo -e "${C2_BG}${C2_FG}  ██████╔╝██║     ███████║██╔██╗ ██║██╔██╗ ██║██║██╔██╗ ██║██║  ███╗${RESET}"
    echo -e "${C2_BG}${C2_FG}  ██╔═══╝ ██║     ██╔══██║██║╚██╗██║██║╚██╗██║██║██║╚██╗██║██║   ██║${RESET}"
    echo -e "${C2_BG}${C2_FG}  ██║     ███████╗██║  ██║██║ ╚████║██║ ╚████║██║██║ ╚████║╚██████╔╝${RESET}"
    echo -e "${C2_BG}${C2_FG}  ╚═╝     ╚══════╝╚═╝  ╚═╝╚═╝  ╚═══╝╚═╝  ╚═══╝╚═╝╚═╝  ╚═══╝ ╚═════╝ ${RESET}"
    echo -e "${C2_BG}${C2_FG}                                                                  ${RESET}"
    echo -e ""
    echo -e "${YELLOW}${BOLD}Phase 2: Implementation Planning${RESET}"
    echo -e "${DIM}Bite-sized tasks → exact paths → TDD steps${RESET}"
    echo -e "${DIM}Colour: ${YELLOW}Yellow${RESET} | ${DIM}Session: ${YELLOW}📋 Planning${RESET}"
    echo -e ""
}

banner_phase3() {
    echo -e ""
    echo -e "${C3_BG}${C3_FG}                                                                  ${RESET}"
    echo -e "${C3_BG}${C3_FG}  ██╗███╗   ███╗██████╗ ██╗     ███████╗███╗   ███╗███████╗███╗   ██╗████████╗${RESET}"
    echo -e "${C3_BG}${C3_FG}  ██║████╗ ████║██╔══██╗██║     ██╔════╝████╗ ████║██╔════╝████╗  ██║╚══██╔══╝${RESET}"
    echo -e "${C3_BG}${C3_FG}  ██║██╔████╔██║██████╔╝██║     █████╗  ██╔████╔██║█████╗  ██╔██╗ ██║   ██║   ${RESET}"
    echo -e "${C3_BG}${C3_FG}  ██║██║╚██╔╝██║██╔═══╝ ██║     ██╔══╝  ██║╚██╔╝██║██╔══╝  ██║╚██╗██║   ██║   ${RESET}"
    echo -e "${C3_BG}${C3_FG}  ██║██║ ╚═╝ ██║██║     ███████╗███████╗██║ ╚═╝ ██║███████╗██║ ╚████║   ██║   ${RESET}"
    echo -e "${C3_BG}${C3_FG}  ╚═╝╚═╝     ╚═╝╚═╝     ╚══════╝╚══════╝╚═╝     ╚═╝╚══════╝╚═╝  ╚═══╝   ╚═╝   ${RESET}"
    echo -e "${C3_BG}${C3_FG}                                                                  ${RESET}"
    echo -e ""
    echo -e "${GREEN}${BOLD}Phase 3: Subagent-Driven Implementation${RESET}"
    echo -e "${DIM}Fresh subagent per task → 2-stage review → green tests${RESET}"
    echo -e "${DIM}Colour: ${GREEN}Green${RESET} | ${DIM}Session: ${GREEN}⚡ Implementation${RESET}"
    echo -e ""
    echo -e "  ${MAGENTA}[SUBAGENT: implementer]${RESET}   — implements task (TDD)"
    echo -e "  ${CYAN}[REVIEW: spec-compliance]${RESET} — verifies against spec"
    echo -e "  ${YELLOW}[REVIEW: code-quality]${RESET}   — style, errors, edge cases"
    echo -e ""
}

# ─── Проверка Claude Code ────────────────────────────────────────────────────
check_claude() {
    if ! command -v claude &>/dev/null; then
        echo -e "\033[31mERROR: 'claude' CLI not found. Install: npm install -g @anthropic-ai/claude-code\033[0m"
        exit 1
    fi
}

# ─── Main ────────────────────────────────────────────────────────────────────
check_claude

PHASE="${1:-}"

case "$PHASE" in
    phase1|1|arch|architecture)
        banner_phase1
        echo -e "${DIM}Load skills/architecture-documentation.md and start Phase 1${RESET}"
        echo -e "${DIM}Run /rename 🏗️ Architecture to name the terminal tab${RESET}"
        echo -e ""
        exec claude "/color blue"
        ;;
    phase2|2|plan|planning)
        banner_phase2
        echo -e "${DIM}Load skills/writing-plans.md and start Phase 2${RESET}"
        echo -e "${DIM}Run /rename 📋 Planning to name the terminal tab${RESET}"
        echo -e ""
        exec claude "/color yellow"
        ;;
    phase3|3|impl|implementation)
        banner_phase3
        echo -e "${DIM}Load skills/subagent-driven-development.md and start Phase 3${RESET}"
        echo -e "${DIM}Run /rename ⚡ Implementation to name the terminal tab${RESET}"
        echo -e ""
        exec claude "/color green"
        ;;
    all|full|pipeline)
        echo -e "${BOLD}Memex Pipeline — Full Run${RESET}"
        echo -e ""
        echo -e "  ${BLUE}Phase 1${RESET}: Architecture  →  ${CYAN}./pipeline.sh phase1${RESET}"
        echo -e "  ${YELLOW}Phase 2${RESET}: Planning      →  ${CYAN}./pipeline.sh phase2${RESET}"
        echo -e "  ${GREEN}Phase 3${RESET}: Implementation →  ${CYAN}./pipeline.sh phase3${RESET}"
        echo -e ""
        echo -e "${DIM}Each phase in a SEPARATE terminal window with its own colour.${RESET}"
        echo -e "${DIM}Run in order. Phase N+1 requires Phase N artifacts.${RESET}"
        ;;
    *)
        echo -e "${BOLD}Memex Pipeline — Usage${RESET}"
        echo -e ""
        echo -e "  ${CYAN}./pipeline.sh phase1${RESET}   ${BLUE}🏗️  Architecture${RESET} (ADR + C4 + AGENTS.md)"
        echo -e "  ${CYAN}./pipeline.sh phase2${RESET}   ${YELLOW}📋 Planning${RESET}     (bite-sized tasks)"
        echo -e "  ${CYAN}./pipeline.sh phase3${RESET}   ${GREEN}⚡ Implementation${RESET} (subagents + review)"
        echo -e "  ${CYAN}./pipeline.sh all${RESET}       Show pipeline overview"
        echo -e ""
        echo -e "Aliases: 1/arch, 2/plan, 3/impl"
        echo -e ""
        echo -e "${DIM}Colour legend:${RESET}"
        echo -e "  ${BLUE}Blue${RESET}   = Architecture (discussion, decisions, diagrams)"
        echo -e "  ${YELLOW}Yellow${RESET} = Planning (tasks, paths, TDD steps)"
        echo -e "  ${GREEN}Green${RESET}  = Implementation (subagents, reviews, commits)"
        exit 1
        ;;
esac

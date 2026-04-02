#!/usr/bin/env bash
#
# Generate code coverage report for the repository.
# Runs bazel coverage for all test targets and generates combined LCOV report.
#
# Usage:
#   ./scripts/coverage.sh [OPTIONS]
#
# Options:
#   --html             Generate HTML coverage report in coverage_html/
#   --file PATTERN     Show coverage for files matching pattern (e.g., "format")
#   --below PCT        Show files with coverage below threshold (e.g., --below 50)
#   --above PCT        Show files with coverage above threshold
#   --all-files        Show coverage for all files (not just worst)
#   --skip-tests       Skip running tests, use existing coverage data
#   --check PCT        Check coverage meets threshold, exit 1 if below (for CI)
#   --verbose          Stream bazel output live (useful for CI)
#   --quiet            Suppress normal output (useful with --check)
#   --exclude PATTERN  Exclude files matching regex from threshold check (repeatable)
#   --targets PATTERN  Bazel target pattern to test (default: auto-discover rust_test targets)
#   --help             Show this help message
#
# Examples:
#   ./scripts/coverage.sh                        # Run coverage for all Rust tests
#   ./scripts/coverage.sh --html                 # Generate HTML report
#   ./scripts/coverage.sh --file format          # Show coverage for format files
#   ./scripts/coverage.sh --below 50             # Show files under 50% coverage
#   ./scripts/coverage.sh --skip-tests --below 30  # Analyze existing data
#   ./scripts/coverage.sh --check 80             # CI check: fail if below 80%
#   ./scripts/coverage.sh --targets "//packages/workshop/..."  # Specific package
#
# Environment:
#   COVERAGE_HTML=1              Alternative way to enable HTML report
#

set -e

_html=0
_help=0
_file_pattern=""
_below_threshold=""
_above_threshold=""
_all_files=0
_skip_tests=0
_check_threshold=""
_quiet=0
_verbose=0
_exclude_patterns=""
_targets=""

while [[ $# -gt 0 ]]; do
    case "$1" in
        --html)        _html=1; shift ;;
        --file)        _file_pattern="$2"; shift 2 ;;
        --below)       _below_threshold="$2"; shift 2 ;;
        --above)       _above_threshold="$2"; shift 2 ;;
        --all-files)   _all_files=1; shift ;;
        --skip-tests)  _skip_tests=1; shift ;;
        --check)       _check_threshold="$2"; shift 2 ;;
        --quiet)       _quiet=1; shift ;;
        --verbose)     _verbose=1; shift ;;
        --exclude)     _exclude_patterns="${_exclude_patterns:+$_exclude_patterns|}$2"; shift 2 ;;
        --targets)     _targets="$2"; shift 2 ;;
        --help|-h)     _help=1; shift ;;
        *)             shift ;;
    esac
done

[[ "${COVERAGE_HTML:-0}" == "1" ]] && _html=1

# shellcheck disable=SC2086  # intentional word-splitting on $_bazel_extra
_bazel_extra="${BAZEL_EXTRA_ARGS:-}"

if [[ $_help -eq 1 ]]; then
    sed -n '2,/^$/p' "$0" | sed 's/^# *//'
    exit 0
fi

# ── Colors (when stdout is a terminal) ────────────────────────────────────────

if [[ -t 1 ]]; then
    _red=$'\033[31m' _grn=$'\033[32m' _ylw=$'\033[33m'
    _bld=$'\033[1m' _dim=$'\033[2m' _rst=$'\033[0m'
else
    _red='' _grn='' _ylw='' _bld='' _dim='' _rst=''
fi

# ── Helpers ───────────────────────────────────────────────────────────────────

# Parse LCOV into TSV: file<TAB>lines_found<TAB>lines_hit<TAB>pct<TAB>uncovered
# This is the ONE place we parse LCOV. Everything downstream operates on this TSV.
_parse_lcov() {
    awk '
    /^SF:/ { file = substr($0, 4) }
    /^LF:/ { lf = substr($0, 4) }
    /^LH:/ { lh = substr($0, 4) }
    /^end_of_record/ {
        if (lf > 0) {
            pct = lh / lf * 100
            printf "%s\t%d\t%d\t%.1f\t%d\n", file, lf, lh, pct, lf - lh
        }
        lf = 0; lh = 0
    }
    ' "$1"
}

# Format TSV as human-readable coverage lines with color.
# Usage: ... | _fmt [pct|impact]
#   pct:    "  87.5% (42/48) src/foo.rs"
#   impact: "   6 uncovered ( 88%) src/foo.rs"
_fmt() {
    awk -F'\t' -v mode="${1:-pct}" \
        -v red="$_red" -v grn="$_grn" -v ylw="$_ylw" -v rst="$_rst" '{
        c = ($4+0 < 50) ? red : ($4+0 < 80) ? ylw : grn
        if (mode == "impact")
            printf "%s%4d uncovered (%3.0f%%)%s %s\n", c, $5, $4, rst, $1
        else
            printf "%s%5.1f%%%s (%d/%d) %s\n", c, $4, rst, $3, $2, $1
    }'
}

# ── Run coverage ──────────────────────────────────────────────────────────────

_output_path="$(bazel info output_path 2>/dev/null)"
_coverage_report="$_output_path/_coverage/_coverage_report.dat"

if [[ $_skip_tests -eq 0 ]]; then
    if [[ -z "$_targets" ]]; then
        # Use //... so bazel coverage discovers test targets itself and
        # respects target_compatible_with (skips platform-incompatible
        # targets like the macOS-only desktop app on Linux CI).
        _targets="//..."
    fi

    _bazel_log=$(mktemp)
    trap 'rm -f "$_stats" "$_bazel_log"' EXIT

    # shellcheck disable=SC2086
    if [[ $_verbose -eq 1 ]]; then
        if ! bazel coverage $_targets $_bazel_extra 2>&1 | tee "$_bazel_log"; then
            exit 1
        fi
    else
        if ! bazel coverage $_targets $_bazel_extra >"$_bazel_log" 2>&1; then
            cat "$_bazel_log" >&2
            exit 1
        fi
    fi
fi

if [[ ! -f "$_coverage_report" ]]; then
    echo "ERROR: Coverage report not found at $_coverage_report"
    echo "Run without --skip-tests to generate coverage data."
    exit 1
fi

# ── Parse once, display many ─────────────────────────────────────────────────

_stats=$(mktemp)
trap 'rm -f "$_stats" "$_bazel_log"' EXIT
_parse_lcov "$_coverage_report" > "$_stats"

# Compute totals (respecting --exclude for threshold checks)
if [[ -n "$_exclude_patterns" ]]; then
    _totals=$(awk -F'\t' -v exc="$_exclude_patterns" '$1 !~ exc' "$_stats")
else
    _totals=$(cat "$_stats")
fi

read -r _lines_hit _lines_found _coverage_pct <<< "$(
    echo "$_totals" | awk -F'\t' '
        { lf += $2; lh += $3 }
        END {
            pct = (lf > 0) ? lh / lf * 100 : 0
            printf "%d %d %.1f", lh, lf, pct
        }
    '
)"

if [[ "${_lines_found:-0}" -gt 0 ]] && [[ $_quiet -eq 0 ]]; then
    _c=$(awk -v p="$_coverage_pct" -v r="$_red" -v y="$_ylw" -v g="$_grn" \
        'BEGIN { print (p+0 < 50) ? r : (p+0 < 80) ? y : g }')

    echo ""
    echo "========================================"
    echo "COVERAGE SUMMARY"
    echo "========================================"
    echo ""
    echo "Total: $_lines_hit / $_lines_found lines (${_c}${_coverage_pct}%${_rst})"
    echo ""

    # Language breakdown
    echo "$_totals" | awk -F'\t' '
        $1 ~ /\.rs$/                         { rs_lf+=$2; rs_lh+=$3 }
        $1 ~ /\.(ts|js|tsx|jsx|svelte)$/     { js_lf+=$2; js_lh+=$3 }
        END {
            if (rs_lf > 0) printf "Rust:       %d / %d lines (%.1f%%)\n", rs_lh, rs_lf, rs_lh/rs_lf*100
            if (js_lf > 0) printf "JS/TS:      %d / %d lines (%.1f%%)\n", js_lh, js_lf, js_lh/js_lf*100
        }
    '

    # Per-file view — each mode is just a filter + sort on the same TSV
    if [[ -n "$_file_pattern" ]]; then
        echo ""; echo "Files matching '$_file_pattern':"
        awk -F'\t' -v p="$_file_pattern" '$1 ~ p' "$_stats" \
            | sort -t$'\t' -k4 -rn | _fmt pct

    elif [[ -n "$_below_threshold" ]]; then
        echo ""; echo "Files below ${_below_threshold}% coverage:"
        awk -F'\t' -v t="$_below_threshold" '$4+0 < t+0' "$_stats" \
            | sort -t$'\t' -k4 -n | _fmt pct

    elif [[ -n "$_above_threshold" ]]; then
        echo ""; echo "Files above ${_above_threshold}% coverage:"
        awk -F'\t' -v t="$_above_threshold" '$4+0 >= t+0' "$_stats" \
            | sort -t$'\t' -k4 -rn | _fmt pct

    elif [[ $_all_files -eq 1 ]]; then
        echo ""; echo "All files:"
        sort -t$'\t' -k4 -rn "$_stats" | _fmt pct

    else
        echo ""; echo "Files with most uncovered lines (sorted by impact):"
        awk -F'\t' '$5 >= 10' "$_stats" \
            | sort -t$'\t' -k5 -rn | head -15 | _fmt impact
    fi
fi

# ── HTML report ───────────────────────────────────────────────────────────────

if [[ $_html -eq 1 ]]; then
    if command -v genhtml &> /dev/null; then
        genhtml "$_coverage_report" --output-directory coverage_html --quiet
        [[ $_quiet -eq 0 ]] && echo "" && echo "HTML report: coverage_html/index.html"
    else
        [[ $_quiet -eq 0 ]] && echo "" && echo "Note: Install lcov for HTML reports (brew install lcov)"
    fi
fi

[[ $_quiet -eq 0 ]] && echo "" && echo "${_dim}LCOV report: $_coverage_report${_rst}"

# ── Threshold check (for CI) ─────────────────────────────────────────────────

if [[ -n "$_check_threshold" ]]; then
    _below=$(awk -v p="${_coverage_pct:-0}" -v t="$_check_threshold" \
        'BEGIN { print (p+0 < t+0) ? 1 : 0 }')
    if [[ "$_below" -eq 1 ]]; then
        [[ $_quiet -eq 0 ]] && echo "" && \
            echo "${_red}FAIL: Coverage ${_coverage_pct}% is below threshold ${_check_threshold}%${_rst}"
        exit 1
    else
        [[ $_quiet -eq 0 ]] && echo "" && \
            echo "${_grn}OK: Coverage ${_coverage_pct}% meets threshold ${_check_threshold}%${_rst}"
    fi
fi

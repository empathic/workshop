"""Bazel transition that forces host_compilation_mode to match compilation_mode.

Tauri's proc macros use #[cfg(debug_assertions)] on struct fields shared between
codegen output and runtime (e.g. ResolvedCommand). The proc macro runs in the
host/exec configuration while the generated code compiles in the target
configuration. If those disagree on debug_assertions the build breaks. This
transition keeps them in sync, scoped to Tauri targets only.
"""

def _match_host_compilation_mode_impl(settings, _attr):
    return {"//command_line_option:host_compilation_mode": settings["//command_line_option:compilation_mode"]}

_match_host_compilation_mode = transition(
    implementation = _match_host_compilation_mode_impl,
    inputs = ["//command_line_option:compilation_mode"],
    outputs = ["//command_line_option:host_compilation_mode"],
)

_TRANSITION_ATTRS = {
    "_allowlist_function_transition": attr.label(
        default = "@bazel_tools//tools/allowlists/function_transition_allowlist",
    ),
}

# --- tauri_binary ---------------------------------------------------------

def _tauri_binary_impl(ctx):
    bin_info = ctx.attr.binary[0][DefaultInfo]
    real_executable = bin_info.files_to_run.executable

    # Executable rules require the executable to be created by the rule itself.
    out = ctx.actions.declare_file(ctx.label.name)
    ctx.actions.symlink(output = out, target_file = real_executable)

    return [DefaultInfo(
        files = depset([out]),
        runfiles = ctx.runfiles(files = [real_executable]).merge(bin_info.default_runfiles),
        executable = out,
    )]

tauri_binary = rule(
    implementation = _tauri_binary_impl,
    executable = True,
    attrs = dict({
        "binary": attr.label(
            mandatory = True,
            executable = True,
            cfg = _match_host_compilation_mode,
        ),
    }, **_TRANSITION_ATTRS),
)

# --- tauri_test -----------------------------------------------------------

def _tauri_test_impl(ctx):
    test_info = ctx.attr.test[0][DefaultInfo]
    real_executable = test_info.files_to_run.executable

    # Symlink so the test executable is owned by this rule.
    out = ctx.actions.declare_file(ctx.label.name)
    ctx.actions.symlink(output = out, target_file = real_executable)

    return [DefaultInfo(
        files = depset([out]),
        runfiles = ctx.runfiles(files = [real_executable]).merge(test_info.default_runfiles),
        executable = out,
    )]

tauri_test = rule(
    implementation = _tauri_test_impl,
    test = True,
    attrs = dict({
        "test": attr.label(
            mandatory = True,
            executable = True,
            cfg = _match_host_compilation_mode,
        ),
    }, **_TRANSITION_ATTRS),
)

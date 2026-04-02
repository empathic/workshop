"""Rule for assembling a macOS .app bundle as a tree artifact."""

def _macos_app_impl(ctx):
    app_dir = ctx.actions.declare_directory(ctx.attr.bundle_name + ".app")

    ctx.actions.run_shell(
        outputs = [app_dir],
        inputs = [ctx.executable.binary, ctx.file.info_plist],
        command = """\
mkdir -p {out}/Contents/MacOS
mkdir -p {out}/Contents/Resources
cp {binary} {out}/Contents/MacOS/{binary_name}
cp {plist} {out}/Contents/Info.plist
""".format(
            out = app_dir.path,
            binary = ctx.executable.binary.path,
            binary_name = ctx.attr.binary_name,
            plist = ctx.file.info_plist.path,
        ),
    )

    return [DefaultInfo(files = depset([app_dir]))]

macos_app = rule(
    implementation = _macos_app_impl,
    attrs = {
        "binary": attr.label(
            mandatory = True,
            executable = True,
            cfg = "target",
            doc = "The main application binary (includes embedded server)",
        ),
        "info_plist": attr.label(
            mandatory = True,
            allow_single_file = [".plist"],
            doc = "The Info.plist file for the app bundle",
        ),
        "bundle_name": attr.string(
            mandatory = True,
            doc = "Name of the .app bundle (e.g. 'Workshop' produces Workshop.app)",
        ),
        "binary_name": attr.string(
            mandatory = True,
            doc = "Filename for the main binary inside Contents/MacOS/",
        ),
    },
)

def rust_source_boundary_test(name, declaration, srcs):
    native.filegroup(
        name = name + "-sources",
        srcs = [declaration] + srcs,
        visibility = ["//visibility:public"],
    )
    native.sh_test(
        name = name,
        srcs = ["//build-support:rust-source-boundary-test.sh"],
        args = [
            "$(rootpath //build-support:rust-source-boundary-check)",
            "$(rootpath " + declaration + ")",
        ],
        data = ["//build-support:rust-source-boundary-check", declaration] + srcs,
        visibility = ["//visibility:public"],
    )

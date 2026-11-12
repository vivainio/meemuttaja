fn main() {
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-changed=build.rs");
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() != Ok("windows") {
        return;
    }

    let version = std::env::var("CARGO_PKG_VERSION").expect("package version");
    let numeric = version.split(['-', '+']).next().unwrap_or(&version);
    let mut parts = numeric
        .split('.')
        .map(|part| part.parse::<u16>().expect("numeric version"));
    let major = parts.next().unwrap_or(0);
    let minor = parts.next().unwrap_or(0);
    let patch = parts.next().unwrap_or(0);
    let icon = std::path::Path::new(&std::env::var("CARGO_MANIFEST_DIR").unwrap())
        .join("assets/icon.ico")
        .to_string_lossy()
        .replace('\\', "/");
    let resource = format!(
        r#"1 ICON "{icon}"
1 VERSIONINFO
FILEVERSION {major},{minor},{patch},0
PRODUCTVERSION {major},{minor},{patch},0
FILEFLAGSMASK 0x3fL
FILEFLAGS 0x0L
FILEOS 0x40004L
FILETYPE 0x1L
FILESUBTYPE 0x0L
BEGIN
    BLOCK "StringFileInfo"
    BEGIN
        BLOCK "040904B0"
        BEGIN
            VALUE "CompanyName", "Ville Vainio\0"
            VALUE "FileDescription", "Meemuttaja meme generator\0"
            VALUE "FileVersion", "{version}\0"
            VALUE "InternalName", "meemuttaja\0"
            VALUE "OriginalFilename", "meemuttaja.exe\0"
            VALUE "ProductName", "Meemuttaja\0"
            VALUE "ProductVersion", "{version}\0"
        END
    END
    BLOCK "VarFileInfo"
    BEGIN
        VALUE "Translation", 0x0409, 1200
    END
END
"#
    );
    let path = std::path::PathBuf::from(std::env::var_os("OUT_DIR").unwrap()).join("meemuttaja.rc");
    std::fs::write(&path, resource).expect("write Windows resource");
    embed_resource::compile(path, embed_resource::NONE)
        .manifest_optional()
        .expect("failed to embed Windows resources");
}

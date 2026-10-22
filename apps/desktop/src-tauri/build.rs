fn main() {
    // 生成 Windows 资源（图标、版本信息），并把 tauri.conf.json 编译进二进制。
    tauri_build::build()
}

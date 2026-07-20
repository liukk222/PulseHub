fn main() {
    slint_build::compile("../../apps/pulsehub-config/ui/app-window.slint")
        .expect("Slint UI 编译失败");
}

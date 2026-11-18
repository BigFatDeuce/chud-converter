//! Intègre l'icône du logiciel (assets/branding/icon.ico) dans chud.exe sous Windows.
//! Si le compilateur de ressources (rc.exe du SDK Windows) est absent, la compilation continue
//! sans icône et un avertissement l'indique.

fn main() {
    println!("cargo:rerun-if-changed=../../assets/branding/icon.ico");
    #[cfg(windows)]
    {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("../../assets/branding/icon.ico");
        res.set("ProductName", "Chud Converter");
        res.set("FileDescription", "Chud Converter — interface en ligne de commande");
        res.set("LegalCopyright", "Contributeurs de Chud Converter — licence GPL-3.0-or-later");
        if let Err(e) = res.compile() {
            println!("cargo:warning=icône non intégrée (compilateur de ressources indisponible) : {e}");
        }
    }
}

use nitidus::logging;

fn main() -> anyhow::Result<()> {
    let _guard = logging::init()?;
    nitidus::run()
}

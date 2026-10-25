use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use std::fs::{self, File, OpenOptions};
use std::io;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};

static COPY_SEQUENCE: AtomicU64 = AtomicU64::new(0);

#[pyclass(frozen)]
pub(crate) struct PrivateMenuCard {
    path: PathBuf,
}

#[pymethods]
impl PrivateMenuCard {
    #[getter]
    fn path(&self) -> String {
        self.path.to_string_lossy().into_owned()
    }
}

fn copy_card(source: &Path, directory: &Path) -> io::Result<PrivateMenuCard> {
    let mut input = File::open(source)?;
    let before = input.metadata()?;
    if !before.is_file() || before.len() == 0 || before.len() % 512 != 0 || before.len() > 4 << 30 {
        return Err(io::Error::new(io::ErrorKind::InvalidInput, "Private card requires a regular whole-sector image up to 4 GiB"));
    }
    fs::create_dir_all(directory)?;
    let name = format!("s2-private-card-{}-{}.img", std::process::id(), COPY_SEQUENCE.fetch_add(1, Ordering::Relaxed));
    let path = directory.join(name);
    let mut output = OpenOptions::new().write(true).create_new(true).open(&path)?;
    let result = (|| {
        let copied = io::copy(&mut input, &mut output)?;
        let after = input.metadata()?;
        if copied != before.len() || before.len() != after.len() || before.modified()? != after.modified()? {
            return Err(io::Error::new(io::ErrorKind::InvalidData, "Card image changed during private copy"));
        }
        Ok(PrivateMenuCard { path: fs::canonicalize(&path)? })
    })();
    if result.is_err() {
        let _ = fs::remove_file(path);
    }
    result
}

#[pyfunction]
fn copy_menu_card(source: PathBuf, directory: PathBuf) -> PyResult<PrivateMenuCard> {
    copy_card(&source, &directory).map_err(|error| PyValueError::new_err(error.to_string()))
}

fn private_media_matches(image: Option<&Path>, private: Option<&PrivateMenuCard>) -> bool {
    match (image, private) {
        (Some(image), Some(private)) => fs::canonicalize(image).ok().as_ref() == Some(&private.path),
        _ => false,
    }
}

#[pyfunction]
fn permit_full_press(board: &Bound<'_, PyAny>) -> PyResult<()> {
    let devices = board.getattr("devices")?;
    let mut present = None;
    let mut image = None;
    for item in devices.try_iter()? {
        let device = item?;
        let name = device.get_type().name()?;
        if name == "BootControls" {
            present = Some(device.call_method1("gpio_input_pin", (12, 7))?.extract::<u8>()? == 0);
        } else if name == "SDCardController" {
            let media = device.getattr("image")?;
            if !media.is_none() {
                image = Some(media.getattr("path")?.extract::<PathBuf>()?);
            }
        }
    }
    if present == Some(false) {
        return Ok(());
    }
    if present == Some(true) {
        if let Ok(value) = board.getattr("_s2_private_card") {
            if let Ok(private) = value.extract::<PyRef<'_, PrivateMenuCard>>() {
                if private_media_matches(image.as_deref(), Some(&private)) {
                    return Ok(());
                }
            }
        }
    }
    Err(PyValueError::new_err("S2 requires no card or a run-owned private card copy; shared card images are refused"))
}

pub(crate) fn register(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<PrivateMenuCard>()?;
    module.add_function(wrap_pyfunction!(copy_menu_card, module)?)?;
    module.add_function(wrap_pyfunction!(permit_full_press, module)?)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command;
    use pyo3::types::{PyDict, PyList, PyTuple};

    struct Fixture(PathBuf);

    impl Fixture {
        fn new() -> Self {
            let path = std::env::temp_dir().join(format!(
                "xm1-s2-media-{}-{}", std::process::id(), COPY_SEQUENCE.fetch_add(1, Ordering::Relaxed)));
            fs::create_dir(&path).unwrap();
            Self(path)
        }
    }

    impl Drop for Fixture {
        fn drop(&mut self) {
            fs::remove_dir_all(&self.0).unwrap();
        }
    }

    fn sha256(path: &Path) -> String {
        let output = Command::new("sha256sum").arg(path).output().unwrap();
        assert!(output.status.success());
        String::from_utf8(output.stdout).unwrap().split_whitespace().next().unwrap().to_owned()
    }

    fn board<'py>(py: Python<'py>, image: &Path, present: bool) -> Bound<'py, PyAny> {
        let namespace = py.import("types").unwrap().getattr("SimpleNamespace").unwrap();
        let mock = py.import("unittest.mock").unwrap().getattr("Mock").unwrap();
        let pin = mock.call0().unwrap();
        pin.setattr("return_value", u8::from(!present)).unwrap();
        let controls = PyDict::new(py);
        controls.set_item("gpio_input_pin", pin).unwrap();
        let make_type = py.import("builtins").unwrap().getattr("type").unwrap();
        let controls = make_type.call1(("BootControls", PyTuple::empty(py), controls)).unwrap().call0().unwrap();
        let media = namespace.call0().unwrap();
        media.setattr("path", image).unwrap();
        let card = PyDict::new(py);
        card.set_item("image", media).unwrap();
        let card = make_type.call1(("SDCardController", PyTuple::empty(py), card)).unwrap().call0().unwrap();
        let board = namespace.call0().unwrap();
        board.setattr("devices", PyList::new(py, [controls, card]).unwrap()).unwrap();
        board
    }

    fn api(py: Python<'_>) -> Bound<'_, PyModule> {
        let module = PyModule::new(py, "_native").unwrap();
        register(&module).unwrap();
        module
    }

    #[test]
    fn s2_refuses_a_shared_card_image() {
        let fixture = Fixture::new();
        let shared = fixture.0.join("shared.img");
        fs::write(&shared, [0x51; 512]).unwrap();
        Python::initialize();
        Python::attach(|py| {
            let api = api(py);
            let board = board(py, &shared, true);
            assert!(api.getattr("permit_full_press").unwrap().call1((&board,)).unwrap_err().is_instance_of::<PyValueError>(py));
            let private = api.getattr("copy_menu_card").unwrap().call1((&shared, &fixture.0)).unwrap();
            board.setattr("_s2_private_card", private).unwrap();
            assert!(api.getattr("permit_full_press").unwrap().call1((&board,)).unwrap_err().is_instance_of::<PyValueError>(py));
        });
    }

    #[test]
    fn private_card_writes_leave_shared_image_sha256_unchanged() {
        let fixture = Fixture::new();
        let shared = fixture.0.join("shared.img");
        fs::write(&shared, [0x51; 512]).unwrap();
        let before = sha256(&shared);
        Python::initialize();
        Python::attach(|py| {
            let api = api(py);
            let private = api.getattr("copy_menu_card").unwrap().call1((&shared, &fixture.0)).unwrap();
            let path = private.getattr("path").unwrap().extract::<PathBuf>().unwrap();
            let board = board(py, &path, true);
            board.setattr("_s2_private_card", private).unwrap();
            api.getattr("permit_full_press").unwrap().call1((&board,)).unwrap();
            fs::write(&path, [0xa4; 512]).unwrap();
            assert_ne!(sha256(&path), before);
            assert_eq!(sha256(&shared), before);
        });
    }

    #[test]
    fn s2_without_a_card_needs_no_private_image() {
        Python::initialize();
        Python::attach(|py| {
            let api = api(py);
            let board = board(py, Path::new("unused.img"), false);
            api.getattr("permit_full_press").unwrap().call1((&board,)).unwrap();
        });
    }
}

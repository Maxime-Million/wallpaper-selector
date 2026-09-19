//! Instance unique et bascule de la fenêtre.
//!
//! Le premier lancement devient le processus résident qui écoute sur
//! `$XDG_RUNTIME_DIR/wallpaper-selector.sock`. Tous les lancements suivants se
//! contentent d'envoyer une commande et de sortir : c'est ce qui rend
//! l'ouverture quasi instantanée (index, vignettes et widgets sont déjà en
//! mémoire).

use std::io::{BufRead, BufReader, Write};
use std::os::unix::net::{UnixListener, UnixStream};
use std::path::Path;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Command {
    Toggle,
    Show,
    Hide,
    Quit,
    Reload,
}

impl Command {
    fn wire(self) -> &'static str {
        match self {
            Command::Toggle => "toggle",
            Command::Show => "show",
            Command::Hide => "hide",
            Command::Quit => "quit",
            Command::Reload => "reload",
        }
    }

    fn parse(s: &str) -> Option<Command> {
        match s.trim() {
            "toggle" => Some(Command::Toggle),
            "show" => Some(Command::Show),
            "hide" => Some(Command::Hide),
            "quit" => Some(Command::Quit),
            "reload" => Some(Command::Reload),
            _ => None,
        }
    }
}

/// Envoie une commande au processus résident.
pub fn send(socket: &Path, cmd: Command) -> std::io::Result<()> {
    let mut stream = UnixStream::connect(socket)?;
    stream.set_write_timeout(Some(std::time::Duration::from_millis(500)))?;
    stream.write_all(cmd.wire().as_bytes())?;
    stream.write_all(b"\n")?;
    stream.flush()?;
    // Le daemon ne répond « ok » qu'une fois l'action réellement exécutée sur
    // la boucle principale GTK. Un script peut donc enchaîner sur un `--toggle`
    // en sachant la fenêtre affichée.
    let mut reader = BufReader::new(stream);
    let mut line = String::new();
    reader.read_line(&mut line)?;
    if line.trim() == "ok" {
        Ok(())
    } else {
        Err(std::io::Error::new(
            std::io::ErrorKind::TimedOut,
            format!("réponse inattendue du daemon : {:?}", line.trim()),
        ))
    }
}

pub enum ServeError {
    /// Un daemon répond déjà sur ce socket.
    AlreadyRunning,
    Io(std::io::Error),
}

impl std::fmt::Display for ServeError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ServeError::AlreadyRunning => write!(f, "une instance répond déjà"),
            ServeError::Io(e) => write!(f, "{e}"),
        }
    }
}

/// Prend possession du socket et traite les commandes dans un thread dédié.
///
/// `handler` est appelé depuis ce thread : il doit renvoyer (si besoin) sur la
/// boucle principale GTK, ce que fait [`crate::ui::on_main`]. Il reçoit un
/// acquittement à appeler une fois l'action terminée ; c'est ce qui déclenche
/// la réponse au client.
pub fn serve<F>(socket: &Path, handler: F) -> Result<(), ServeError>
where
    F: Fn(Command, Box<dyn FnOnce() + Send>) + Send + 'static,
{
    // Un socket resté sur le disque après un crash ne doit pas bloquer le
    // démarrage : on teste d'abord s'il y a vraiment quelqu'un en face.
    if socket.exists() && UnixStream::connect(socket).is_ok() {
        return Err(ServeError::AlreadyRunning);
    }
    if socket.exists() {
        let _ = std::fs::remove_file(socket);
    }
    if let Some(parent) = socket.parent() {
        let _ = std::fs::create_dir_all(parent);
    }

    let listener = UnixListener::bind(socket).map_err(ServeError::Io)?;

    std::thread::spawn(move || {
        for stream in listener.incoming() {
            let Ok(stream) = stream else { continue };
            let mut line = String::new();
            if BufReader::new(&stream).read_line(&mut line).is_err() {
                continue;
            }
            let Some(cmd) = Command::parse(&line) else {
                continue;
            };

            let (tx, rx) = std::sync::mpsc::channel();
            let ack: Box<dyn FnOnce() + Send> = Box::new(move || {
                let _ = tx.send(());
            });
            handler(cmd, ack);

            let mut s = &stream;
            let reply: &[u8] = if rx.recv_timeout(std::time::Duration::from_secs(3)).is_ok() {
                b"ok\n"
            } else {
                b"timeout\n"
            };
            let _ = s.write_all(reply);
        }
    });

    Ok(())
}

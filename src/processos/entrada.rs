//! A entrada de um processo filho escrita numa thread própria.
//!
//! O pipe da entrada guarda 64 KiB; passou disso, a escrita espera o processo ler. O Claude Code
//! só começa a ler a entrada depois de subir (medido em 19/11/2026: 1,5 s com o Node quente, 5 s
//! frio, mais os hooks e os MCP da pasta), e uma mensagem com um print anexado tem mais de 1 MB em
//! base64. Escrita na thread da interface, ela segurava a janela esse tempo todo e o GNOME
//! perguntava se era para forçar a saída (dono, 19/11/2026: "de vez em quando mando uma mensagem
//! e dá isso"). Aqui a escrita só entra numa fila e volta na hora; a thread escreve na ordem e fecha
//! a entrada do processo quando esta sai de cena, depois do que já estava na fila.

use std::{
    io::{self, Write},
    sync::mpsc,
};

pub struct EntradaDeFundo {
    fila: mpsc::Sender<Vec<u8>>,
    /// O que foi escrito desde o último `flush`: uma linha do protocolo vai inteira para a fila.
    pendente: Vec<u8>,
}

impl EntradaDeFundo {
    pub fn nova(mut destino: impl Write + Send + 'static) -> Self {
        let (fila, recebidos) = mpsc::channel::<Vec<u8>>();
        std::thread::spawn(move || {
            for bloco in recebidos {
                // O processo fechou a entrada (ou saiu): o resto da fila não tem para onde ir, e a
                // próxima escrita de quem chamou volta BrokenPipe.
                if destino.write_all(&bloco).and_then(|()| destino.flush()).is_err() {
                    return;
                }
            }
        });
        Self { fila, pendente: Vec::new() }
    }
}

impl Write for EntradaDeFundo {
    fn write(&mut self, bytes: &[u8]) -> io::Result<usize> {
        self.pendente.extend_from_slice(bytes);
        Ok(bytes.len())
    }

    fn flush(&mut self) -> io::Result<()> {
        if self.pendente.is_empty() {
            return Ok(());
        }
        self.fila
            .send(std::mem::take(&mut self.pendente))
            .map_err(|_| io::Error::new(io::ErrorKind::BrokenPipe, "a entrada do processo fechou"))
    }
}

impl Drop for EntradaDeFundo {
    fn drop(&mut self) {
        let _ = self.flush();
    }
}

#[cfg(test)]
mod testes {
    use super::*;
    use std::{
        io::Read,
        time::{Duration, Instant},
    };

    /// Ninguém lendo o pipe: 2 MB entram na hora, e chegam inteiros e na ordem quando o outro lado
    /// lê, com o fim da entrada depois deles.
    #[test]
    fn escrever_nao_espera_o_processo_ler() {
        let (mut leitura, escrita) = io::pipe().unwrap();
        let mut entrada = EntradaDeFundo::nova(escrita);
        let linhas: Vec<Vec<u8>> = (0..4u8).map(|n| vec![b'a' + n; 512 * 1024]).collect();
        let inicio = Instant::now();
        for linha in &linhas {
            entrada.write_all(linha).unwrap();
            entrada.flush().unwrap();
        }
        drop(entrada);
        assert!(inicio.elapsed() < Duration::from_millis(500), "a escrita esperou o pipe: {:?}", inicio.elapsed());
        let mut lido = Vec::new();
        leitura.read_to_end(&mut lido).unwrap();
        assert_eq!(lido, linhas.concat());
    }

    /// Com o processo sem entrada, a escrita seguinte volta erro (como voltava a escrita direta).
    #[test]
    fn entrada_fechada_volta_erro() {
        let (leitura, escrita) = io::pipe().unwrap();
        drop(leitura);
        let mut entrada = EntradaDeFundo::nova(escrita);
        let limite = Instant::now() + Duration::from_secs(5);
        let mut resultado = Ok(());
        while resultado.is_ok() && Instant::now() < limite {
            resultado = entrada.write_all(b"oi\n").and_then(|()| entrada.flush());
            std::thread::sleep(Duration::from_millis(10));
        }
        assert_eq!(resultado.unwrap_err().kind(), io::ErrorKind::BrokenPipe);
    }
}

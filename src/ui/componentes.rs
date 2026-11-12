//! Componentes da linguagem visual do Vitral (o mini design system): cada peça tem um desenho
//! só, usado em todas as telas. `vitral --vitrine` mostra todos em todos os estados.

mod base;
mod codigo;
mod controles;
mod dialogo;
mod estado;
mod graficos;
mod listas;
mod malha;
pub mod metal;
mod paineis;
mod superficies;
mod textos;
pub mod vitrine;

pub use base::*;
pub use codigo::*;
pub use controles::*;
pub use dialogo::*;
pub use estado::*;
pub use graficos::*;
pub use listas::*;
pub use malha::*;
pub use paineis::*;
pub use superficies::*;
pub use textos::*;

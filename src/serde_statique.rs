//! Lecture JSON (serde) des champs `&'static str` (rôles, libellés...).
//!
//! Un `&'static str` est un texte qui vit pendant tout le programme : en temps
//! normal il vient d'une constante du code. Quand l'état arrive en JSON (processus
//! de la fenêtre), il faut en fabriquer : on « interne » chaque texte DISTINCT une
//! seule fois (gardé à vie dans une table). Les rôles et libellés sont en nombre
//! fini : la mémoire reste bornée, même après des milliers de messages.
//!
//! Utilisation sur un champ : `#[serde(deserialize_with = "crate::serde_statique::texte")]`.

use std::collections::HashSet;
use std::sync::Mutex;

use serde::{Deserialize, Deserializer};

/// Même type que `&'static str`, sous un autre nom. Utile car `#[derive(Deserialize)]`
/// voit le mot `'static` dans un champ et exige alors que TOUT le JSON vive à vie ;
/// avec ce nom, il ne le voit plus et notre `deserialize_with` fait le travail.
pub type Texte = &'static str;

/// Renvoie la version « à vie » d'un texte (créée la première fois seulement).
pub fn interner(s: &str) -> &'static str {
    static TABLE: Mutex<Option<HashSet<&'static str>>> = Mutex::new(None);
    let mut table = TABLE.lock().unwrap();
    let table = table.get_or_insert_with(HashSet::new);
    if let Some(&deja) = table.get(s) {
        return deja;
    }
    // `Box::leak` : on renonce à libérer ce texte, il devient `'static`.
    let a_vie: &'static str = Box::leak(s.to_string().into_boxed_str());
    table.insert(a_vie);
    a_vie
}

pub fn texte<'de, D: Deserializer<'de>>(d: D) -> Result<&'static str, D::Error> {
    Ok(interner(&String::deserialize(d)?))
}

pub fn option<'de, D: Deserializer<'de>>(d: D) -> Result<Option<&'static str>, D::Error> {
    Ok(Option::<String>::deserialize(d)?.map(|s| interner(&s)))
}

/// `Vec<(&'static str, T)>` (ex : stats par rôle).
pub fn paires<'de, D: Deserializer<'de>, T: Deserialize<'de>>(d: D) -> Result<Vec<(&'static str, T)>, D::Error> {
    Ok(Vec::<(String, T)>::deserialize(d)?.into_iter().map(|(k, v)| (interner(&k), v)).collect())
}

/// `Vec<(&'static str, &'static str)>` (ex : rôles dans la compo).
pub fn paires_textes<'de, D: Deserializer<'de>>(d: D) -> Result<Vec<(&'static str, &'static str)>, D::Error> {
    Ok(Vec::<(String, String)>::deserialize(d)?.into_iter().map(|(a, b)| (interner(&a), interner(&b))).collect())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn un_texte_n_est_garde_qu_une_fois() {
        let a = interner("middle");
        let b = interner(&String::from("middle"));
        assert!(std::ptr::eq(a, b));
        assert_eq!(a, "middle");
    }
}

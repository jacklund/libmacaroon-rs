use error::MacaroonError;
use sodiumoxide::crypto::secretbox;
use sodiumoxide::crypto::auth::hmacsha256::{self, Tag, Key};
use std::str;
use super::serialization;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Caveat {
    pub id: String,
    pub verifier_id: Option<Vec<u8>>,
    pub location: Option<String>,
}

impl Caveat {
    pub fn new(id: String,
               verifier_id: Option<Vec<u8>>,
               location: Option<String>)
               -> Result<Caveat, MacaroonError> {
        let caveat: Caveat = Caveat {
            id: id,
            verifier_id: verifier_id,
            location: location,
        };

        caveat.validate()
    }

    pub fn validate(self) -> Result<Self, MacaroonError> {
        if self.id.is_empty() {
            return Err(MacaroonError::BadMacaroon("Caveat with no identifier"));
        }

        Ok(self)
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Macaroon {
    pub location: Option<String>,
    pub identifier: String,
    pub signature: Vec<u8>,
    pub caveats: Vec<Caveat>,
}

const KEY_GENERATOR: &'static [u8; 32] = b"macaroons-key-generator\0\0\0\0\0\0\0\0\0";

impl Macaroon {
    fn generate_derived_key(key: &[u8; 32]) -> Result<[u8; 32], MacaroonError> {
        hmac_vec(&KEY_GENERATOR.to_vec(), key)
    }

    pub fn create(location: &'static str,
                  key: &[u8; 32],
                  identifier: &'static str)
                  -> Result<Macaroon, MacaroonError> {
        let derived_key = Macaroon::generate_derived_key(&key)?;

        let macaroon: Macaroon = Macaroon {
            location: Some(String::from(location)),
            identifier: String::from(identifier),
            signature: hmac(&derived_key, identifier.as_bytes()).to_vec(),
            caveats: Vec::new(),
        };
        macaroon.validate()
    }

    pub fn validate(self) -> Result<Self, MacaroonError> {
        if self.identifier.is_empty() {
            return Err(MacaroonError::BadMacaroon("No macaroon identifier"));
        }
        if self.signature.is_empty() {
            return Err(MacaroonError::BadMacaroon("No macaroon signature"));
        }

        Ok(self)
    }

    #[allow(unused_variables)]
    pub fn verify(&self, verifier: &Verifier) -> Result<bool, MacaroonError> {
        Ok(true)
    }

    pub fn add_first_party_caveat(&mut self, predicate: &'static str) -> Result<(), MacaroonError> {
        self.signature = try!(hmac_vec(&self.signature, predicate.as_bytes())).to_vec();
        self.caveats.push(Caveat::new(String::from(predicate), None, None)?);
        Ok(())
    }

    pub fn add_third_party_caveat(&mut self, location: &str, key: &[u8; 32], id: &str) -> Result<(), MacaroonError> {
        let derived_key: [u8; 32] = Macaroon::generate_derived_key(key)?;
        let vid: Vec<u8> = secretbox::seal(self.signature.as_slice(), &secretbox::gen_nonce(), &secretbox::Key(derived_key));
        let signature = hmac2(&self.signature, &vid, id.as_bytes())?.to_vec();
        self.caveats.push(Caveat::new(String::from(id), Some(vid), Some(String::from(location)))?);
        self.signature = signature;
        Ok(())
    }

    pub fn serialize(&self, format: serialization::Format) -> Result<Vec<u8>, MacaroonError> {
        match format {
            serialization::Format::V1 => serialization::v1::serialize_v1(self),
            serialization::Format::V2 => serialization::v2::serialize_v2(self),
            serialization::Format::V2J => serialization::v2j::serialize_v2j(self),
        }
    }

    pub fn deserialize(data: &Vec<u8>) -> Result<Macaroon, MacaroonError> {
        let macaroon: Macaroon = match data[0] as char {
            '{' => serialization::v2j::deserialize_v2j(data)?,
            '\x02' => serialization::v2::deserialize_v2(data)?,
            'a'...'z' | 'A'...'Z' | '0'...'9' | '+' | '-' | '/' | '_' => serialization::v1::deserialize_v1(data)?,
            _ => return Err(MacaroonError::UnknownSerialization),
        };
        macaroon.validate()
    }
}

pub type VerifierCallback = fn(&Caveat) -> Result<bool, MacaroonError>;

pub struct Verifier {
    predicates: Vec<String>,
    callbacks: Vec<VerifierCallback>,
}

impl Verifier {
    pub fn new() -> Verifier {
        Verifier {
            predicates: Vec::new(),
            callbacks: Vec::new(),
        }
    }
}

fn hmac_vec<'r>(key: &'r Vec<u8>, text: &'r [u8]) -> Result<[u8; 32], MacaroonError> {
    if key.len() != 32 {
        return Err(MacaroonError::KeyError("Wrong key length"));
    }
    let mut key_static: [u8; 32] = [0; 32];
    for i in 0..key.len() {
        key_static[i] = key[i];
    }
    Ok(hmac(&key_static, text))
}

fn hmac<'r>(key: &'r [u8; 32], text: &'r [u8]) -> [u8; 32] {
    let Tag(result_bytes) = hmacsha256::authenticate(text, &Key(*key));
    result_bytes
}

fn hmac2<'r>(key: &'r Vec<u8>, text1: &'r [u8], text2: &'r [u8]) -> Result<[u8; 32], MacaroonError> {
    let tmp1: [u8;32] = hmac_vec(key, text1)?;
    let tmp2: [u8;32] = hmac_vec(key, text2)?;
    let tmp = [tmp1, tmp2].concat();
    hmac_vec(key, &tmp)
}

#[cfg(test)]
mod tests {
    use super::Macaroon;
    use error::MacaroonError;

    #[test]
    fn create_macaroon() {
        let signature = [142, 227, 10, 28, 80, 115, 181, 176, 112, 56, 115, 95, 128, 156, 39, 20,
                         135, 17, 207, 204, 2, 80, 90, 249, 68, 40, 100, 60, 47, 220, 5, 224];
        let key: &[u8; 32] = b"this is a super duper secret key";
        let macaroon_res = Macaroon::create("location", key, "identifier");
        assert!(macaroon_res.is_ok());
        let macaroon = macaroon_res.unwrap();
        assert!(macaroon.location.is_some());
        assert_eq!("location", macaroon.location.unwrap());
        assert_eq!("identifier", macaroon.identifier);
        assert_eq!(signature.to_vec(), macaroon.signature);
        assert_eq!(0, macaroon.caveats.len());
    }

    #[test]
    fn create_invalid_macaroon() {
        let key: &[u8; 32] = b"this is a super duper secret key";
        let macaroon_res: Result<Macaroon, MacaroonError> = Macaroon::create("location", key, "");
        assert!(macaroon_res.is_err());
        let mut macaroon: Macaroon = Macaroon::create("location", key, "identifier").unwrap();
        let macaroon_res = macaroon.add_first_party_caveat("");
        assert!(macaroon_res.is_err());
    }

    #[test]
    fn create_macaroon_with_first_party_caveat() {
        let signature = [132, 133, 51, 243, 147, 201, 178, 7, 193, 179, 36, 128, 4, 228, 17, 84,
                         166, 81, 30, 152, 15, 51, 47, 33, 196, 60, 20, 109, 163, 151, 133, 18];
        let key: &[u8; 32] = b"this is a super duper secret key";
        let mut macaroon = Macaroon::create("location", key, "identifier").unwrap();
        let cav_result = macaroon.add_first_party_caveat("predicate");
        assert!(cav_result.is_ok());
        assert_eq!(1, macaroon.caveats.len());
        let ref caveat = macaroon.caveats[0];
        assert_eq!("predicate", caveat.id);
        assert_eq!(None, caveat.verifier_id);
        assert_eq!(None, caveat.location);
        assert_eq!(signature.to_vec(), macaroon.signature);
    }
}
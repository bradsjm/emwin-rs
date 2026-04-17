//! Structured parsed artifact emitted for a product when a specialized parser matches.

#![allow(missing_docs)]

use crate::{
    Cf6Bulletin, CliBulletin, CwaBulletin, DcpBulletin, DsmBulletin, EroBulletin, FdBulletin,
    HmlBulletin, LsrBulletin, McdBulletin, MetarBulletin, MosBulletin, PirepBulletin, SawBulletin,
    SelBulletin, SigmetBulletin, SpcOutlookBulletin, TafBulletin, WwpBulletin,
};
use serde::Serialize;
use serde_json::Value;

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum ProductArtifact {
    Metar(MetarBulletin),
    Taf(TafBulletin),
    Dcp(DcpBulletin),
    Fd(FdBulletin),
    Pirep(PirepBulletin),
    Sigmet(SigmetBulletin),
    Lsr(LsrBulletin),
    Cli(CliBulletin),
    Cwa(CwaBulletin),
    Wwp(WwpBulletin),
    Saw(SawBulletin),
    Sel(SelBulletin),
    Cf6(Cf6Bulletin),
    Dsm(DsmBulletin),
    Hml(HmlBulletin),
    Mos(MosBulletin),
    Mcd(McdBulletin),
    Ero(EroBulletin),
    SpcOutlook(SpcOutlookBulletin),
}

impl ProductArtifact {
    pub fn kind(&self) -> &'static str {
        match self {
            Self::Metar(_) => "metar",
            Self::Taf(_) => "taf",
            Self::Dcp(_) => "dcp",
            Self::Fd(_) => "fd",
            Self::Pirep(_) => "pirep",
            Self::Sigmet(_) => "sigmet",
            Self::Lsr(_) => "lsr",
            Self::Cli(_) => "cli",
            Self::Cwa(_) => "cwa",
            Self::Wwp(_) => "wwp",
            Self::Saw(_) => "saw",
            Self::Sel(_) => "sel",
            Self::Cf6(_) => "cf6",
            Self::Dsm(_) => "dsm",
            Self::Hml(_) => "hml",
            Self::Mos(_) => "mos",
            Self::Mcd(_) => "mcd",
            Self::Ero(_) => "ero",
            Self::SpcOutlook(_) => "spc_outlook",
        }
    }

    pub fn detail_json(&self) -> Value {
        match self {
            Self::Metar(value) => serde_json::to_value(value),
            Self::Taf(value) => serde_json::to_value(value),
            Self::Dcp(value) => serde_json::to_value(value),
            Self::Fd(value) => serde_json::to_value(value),
            Self::Pirep(value) => serde_json::to_value(value),
            Self::Sigmet(value) => serde_json::to_value(value),
            Self::Lsr(value) => serde_json::to_value(value),
            Self::Cli(value) => serde_json::to_value(value),
            Self::Cwa(value) => serde_json::to_value(value),
            Self::Wwp(value) => serde_json::to_value(value),
            Self::Saw(value) => serde_json::to_value(value),
            Self::Sel(value) => serde_json::to_value(value),
            Self::Cf6(value) => serde_json::to_value(value),
            Self::Dsm(value) => serde_json::to_value(value),
            Self::Hml(value) => serde_json::to_value(value),
            Self::Mos(value) => serde_json::to_value(value),
            Self::Mcd(value) => serde_json::to_value(value),
            Self::Ero(value) => serde_json::to_value(value),
            Self::SpcOutlook(value) => serde_json::to_value(value),
        }
        .unwrap_or(Value::Null)
    }

    pub fn as_metar(&self) -> Option<&MetarBulletin> {
        match self {
            Self::Metar(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_taf(&self) -> Option<&TafBulletin> {
        match self {
            Self::Taf(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_dcp(&self) -> Option<&DcpBulletin> {
        match self {
            Self::Dcp(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_fd(&self) -> Option<&FdBulletin> {
        match self {
            Self::Fd(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_pirep(&self) -> Option<&PirepBulletin> {
        match self {
            Self::Pirep(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_sigmet(&self) -> Option<&SigmetBulletin> {
        match self {
            Self::Sigmet(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_lsr(&self) -> Option<&LsrBulletin> {
        match self {
            Self::Lsr(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_cli(&self) -> Option<&CliBulletin> {
        match self {
            Self::Cli(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_cwa(&self) -> Option<&CwaBulletin> {
        match self {
            Self::Cwa(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_wwp(&self) -> Option<&WwpBulletin> {
        match self {
            Self::Wwp(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_saw(&self) -> Option<&SawBulletin> {
        match self {
            Self::Saw(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_sel(&self) -> Option<&SelBulletin> {
        match self {
            Self::Sel(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_cf6(&self) -> Option<&Cf6Bulletin> {
        match self {
            Self::Cf6(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_dsm(&self) -> Option<&DsmBulletin> {
        match self {
            Self::Dsm(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_hml(&self) -> Option<&HmlBulletin> {
        match self {
            Self::Hml(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_mos(&self) -> Option<&MosBulletin> {
        match self {
            Self::Mos(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_mcd(&self) -> Option<&McdBulletin> {
        match self {
            Self::Mcd(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_ero(&self) -> Option<&EroBulletin> {
        match self {
            Self::Ero(value) => Some(value),
            _ => None,
        }
    }

    pub fn as_spc_outlook(&self) -> Option<&SpcOutlookBulletin> {
        match self {
            Self::SpcOutlook(value) => Some(value),
            _ => None,
        }
    }
}

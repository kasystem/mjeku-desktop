//! Katalogu fiskal i PROGRAMERI POS `SefAtkCodes.cs`.
//! Ky katalog është vetëm-lexim; komandat e ruajtjes e validojnë çdo kod këtu.

use serde::Serialize;

#[derive(Debug, Clone, Serialize)]
pub struct CatalogEntry {
    pub code: &'static str,
    pub label: &'static str,
}

#[derive(Debug, Clone, Serialize)]
pub struct AtkCatalog {
    pub tax_codes: Vec<CatalogEntry>,
    pub category_codes: Vec<CatalogEntry>,
    pub unit_codes: Vec<CatalogEntry>,
    pub stock_kinds: Vec<CatalogEntry>,
}

const TAX_CODES: &[CatalogEntry] = &[
    CatalogEntry {
        code: "A",
        label: "0% — E përjashtuar",
    },
    CatalogEntry {
        code: "C",
        label: "0% — Jashtë TVSH-së",
    },
    CatalogEntry {
        code: "D",
        label: "8%",
    },
    CatalogEntry {
        code: "E",
        label: "18%",
    },
];

const CATEGORY_CODES: &[CatalogEntry] = &[
    CatalogEntry {
        code: "TT",
        label: "Të tjera",
    },
    CatalogEntry {
        code: "UR",
        label: "Ushqime të përgatitura në restorante dhe kuzhina",
    },
    CatalogEntry {
        code: "AU",
        label: "Artikuj ushqimorë nga dyqanet me pakicë",
    },
    CatalogEntry {
        code: "VM",
        label: "Veshmbathje, rroba dhe këpucë",
    },
    CatalogEntry {
        code: "KO",
        label: "Produkte kozmetike dhe materiale për zbukurim",
    },
    CatalogEntry {
        code: "AP",
        label: "Autopjesë nga autoserviset dhe dyqanet e autopjesëve",
    },
    CatalogEntry {
        code: "BM",
        label: "Barna dhe ilaçe mjekësore",
    },
    CatalogEntry {
        code: "PM",
        label: "Pajisje dhe aparatura mjekësore / Produkte të miellit",
    },
    CatalogEntry {
        code: "PP",
        label: "Pemë dhe perime",
    },
    CatalogEntry {
        code: "PE",
        label: "Pajisje elektronike, audio dhe video",
    },
    CatalogEntry {
        code: "PS",
        label: "Pajisje shtëpiake",
    },
    CatalogEntry {
        code: "PB",
        label: "Pajisje dhe makineri bujqësore",
    },
    CatalogEntry {
        code: "BB",
        label: "Barna bujqësore",
    },
    CatalogEntry {
        code: "KB",
        label: "Karburante",
    },
    CatalogEntry {
        code: "EE",
        label: "Energji elektrike",
    },
    CatalogEntry {
        code: "PSE",
        label: "Pajisje speciale elektrike",
    },
    CatalogEntry {
        code: "SM",
        label: "Shërbime mjekësore",
    },
    CatalogEntry {
        code: "SJ",
        label: "Shërbime juridike",
    },
    CatalogEntry {
        code: "ST",
        label: "Shërbime të tjera",
    },
    CatalogEntry {
        code: "SK",
        label: "Shërbime të kontabilitetit",
    },
    CatalogEntry {
        code: "SMK",
        label: "Shërbime mekanike",
    },
    CatalogEntry {
        code: "STR",
        label: "Shërbime transporti",
    },
    CatalogEntry {
        code: "PI",
        label: "Pajisje dhe makineri industriale",
    },
    CatalogEntry {
        code: "PT",
        label: "Mjete/pajisje transporti",
    },
    CatalogEntry {
        code: "SEA",
        label: "Shërbime edukative dhe arsimore",
    },
    CatalogEntry {
        code: "KS",
        label: "Klube sportive",
    },
    CatalogEntry {
        code: "MN",
        label: "Materiale ndërtimore",
    },
    CatalogEntry {
        code: "SHN",
        label: "Shërbime ndërtimore",
    },
    CatalogEntry {
        code: "SDG",
        label: "Shtyp dhe dizajn grafik",
    },
    CatalogEntry {
        code: "HT",
        label: "Hoteleri dhe turizëm",
    },
    CatalogEntry {
        code: "SHB",
        label: "Shërbime bankare",
    },
    CatalogEntry {
        code: "STI",
        label: "Shërbime të teknologjisë informative",
    },
    CatalogEntry {
        code: "SHT",
        label: "Shërbime të telekomunikimit",
    },
    CatalogEntry {
        code: "PBS",
        label: "Prodhimi bimor dhe shtazor",
    },
    CatalogEntry {
        code: "MPM",
        label: "Mish dhe produkte të mishit",
    },
    CatalogEntry {
        code: "PPD",
        label: "Produkte të drurit",
    },
    CatalogEntry {
        code: "PPH",
        label: "Produkte të hekurit",
    },
    CatalogEntry {
        code: "PPP",
        label: "Produkte të plastikës",
    },
    CatalogEntry {
        code: "PPA",
        label: "Produkte të aluminit",
    },
    CatalogEntry {
        code: "PHN",
        label: "Pajisje hidraulike dhe të ngrohjes",
    },
    CatalogEntry {
        code: "SHH",
        label: "Shërbime hidraulike",
    },
    CatalogEntry {
        code: "SUA",
        label: "Shërbime udhëtimi dhe akomodimi",
    },
    CatalogEntry {
        code: "SHZ",
        label: "Shërbime të zbukurimit",
    },
    CatalogEntry {
        code: "PJA",
        label: "Pije joalkoolike",
    },
    CatalogEntry {
        code: "PAL2",
        label: "Pije alkoolike",
    },
    CatalogEntry {
        code: "DUH",
        label: "Duhan",
    },
    CatalogEntry {
        code: "SHM",
        label: "Shërbime marketingu",
    },
    CatalogEntry {
        code: "PRO",
        label: "Prodhimtari",
    },
    CatalogEntry {
        code: "SFV",
        label: "Shpërndarje filmash/videosh/programesh televizive",
    },
    CatalogEntry {
        code: "BSH",
        label: "Blegtori dhe shpezëtari",
    },
    CatalogEntry {
        code: "RPK",
        label: "Riparimi i pajisjeve të komunikimit",
    },
    CatalogEntry {
        code: "PTI",
        label: "Pajisje të teknologjisë informative",
    },
];

const UNIT_CODES: &[CatalogEntry] = &[
    CatalogEntry {
        code: "EA",
        label: "Njësi",
    },
    CatalogEntry {
        code: "CP",
        label: "Copë",
    },
    CatalogEntry {
        code: "LTR",
        label: "Litër",
    },
    CatalogEntry {
        code: "SEC",
        label: "Sekondë",
    },
    CatalogEntry {
        code: "MIN",
        label: "Minutë",
    },
    CatalogEntry {
        code: "HUR",
        label: "Orë",
    },
    CatalogEntry {
        code: "DAY",
        label: "Ditë",
    },
    CatalogEntry {
        code: "MON",
        label: "Muaj",
    },
    CatalogEntry {
        code: "ANN",
        label: "Vite",
    },
    CatalogEntry {
        code: "KGB",
        label: "Kilogram bruto",
    },
    CatalogEntry {
        code: "KGN",
        label: "Kilogram neto",
    },
    CatalogEntry {
        code: "TN",
        label: "Ton",
    },
    CatalogEntry {
        code: "GRM",
        label: "Gram",
    },
    CatalogEntry {
        code: "M4",
        label: "Vlerë monetare",
    },
    CatalogEntry {
        code: "UDH",
        label: "Udhëtim",
    },
    CatalogEntry {
        code: "KMT",
        label: "Km",
    },
    CatalogEntry {
        code: "LM",
        label: "Metër linear",
    },
    CatalogEntry {
        code: "MTS",
        label: "M2",
    },
    CatalogEntry {
        code: "MIV",
        label: "M3",
    },
    CatalogEntry {
        code: "MSK",
        label: "M/S2",
    },
    CatalogEntry {
        code: "KEH",
        label: "Këh (Fuqi)",
    },
    CatalogEntry {
        code: "NFJ",
        label: "Numri i fjalëve",
    },
    CatalogEntry {
        code: "NFQ",
        label: "Numri i faqeve",
    },
    CatalogEntry {
        code: "PRQ",
        label: "Përqindje",
    },
    CatalogEntry {
        code: "DP",
        label: "Ditë pune",
    },
    CatalogEntry {
        code: "OP",
        label: "Orë pune",
    },
    CatalogEntry {
        code: "PRD",
        label: "Përdorim",
    },
    CatalogEntry {
        code: "PAL",
        label: "Palë",
    },
    CatalogEntry {
        code: "XBX",
        label: "Kuti",
    },
    CatalogEntry {
        code: "PK",
        label: "Pako",
    },
    CatalogEntry {
        code: "SET",
        label: "Set",
    },
];

const STOCK_KINDS: &[CatalogEntry] = &[
    CatalogEntry {
        code: "unknown",
        label: "Pa caktuar (unknown)",
    },
    CatalogEntry {
        code: "service",
        label: "Shërbim pa stok (service)",
    },
    CatalogEntry {
        code: "raw",
        label: "Lëndë e parë (raw)",
    },
    CatalogEntry {
        code: "sale",
        label: "Produkt për shitje/prodhim (sale)",
    },
    CatalogEntry {
        code: "resale",
        label: "Produkt për rishitje (resale)",
    },
    CatalogEntry {
        code: "both",
        label: "Lëndë e parë dhe mall (both)",
    },
];

pub fn get() -> AtkCatalog {
    AtkCatalog {
        tax_codes: TAX_CODES.to_vec(),
        category_codes: CATEGORY_CODES.to_vec(),
        unit_codes: UNIT_CODES.to_vec(),
        stock_kinds: STOCK_KINDS.to_vec(),
    }
}

pub fn valid_tax_code(code: &str) -> bool {
    TAX_CODES.iter().any(|x| x.code == code)
}

pub fn tax_percent(code: &str) -> Option<u8> {
    match code {
        "A" | "C" => Some(0),
        "D" => Some(8),
        "E" => Some(18),
        _ => None,
    }
}

pub fn valid_category_code(code: &str) -> bool {
    CATEGORY_CODES.iter().any(|x| x.code == code)
}

pub fn valid_unit_code(code: &str) -> bool {
    UNIT_CODES.iter().any(|x| x.code == code)
}

pub fn valid_stock_kind(code: &str) -> bool {
    STOCK_KINDS.iter().any(|x| x.code == code)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn catalog_matches_pos_codes_and_tax_codes_stay_distinct() {
        let catalog = get();
        assert_eq!(
            catalog.tax_codes.iter().map(|x| x.code).collect::<Vec<_>>(),
            vec!["A", "C", "D", "E"]
        );
        assert_eq!(catalog.category_codes.len(), 52);
        assert_eq!(catalog.unit_codes.len(), 31);
        assert_eq!(
            catalog
                .stock_kinds
                .iter()
                .map(|x| x.code)
                .collect::<Vec<_>>(),
            vec!["unknown", "service", "raw", "sale", "resale", "both"]
        );
        assert_eq!(tax_percent("A"), Some(0));
        assert_eq!(tax_percent("C"), Some(0));
        assert_ne!(catalog.tax_codes[0].code, catalog.tax_codes[1].code);
        assert!(valid_category_code("SM"));
        assert!(valid_unit_code("EA"));
        assert!(!valid_unit_code("not-a-unit"));
    }
}

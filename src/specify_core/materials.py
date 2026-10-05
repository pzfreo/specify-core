"""Common machined and 3-D printed materials, as CNC and printing services
offer them, for suggesting as the part's material is typed. A suggestion, not a
whitelist: any material may be written. Each is (name as it goes on the
drawing, family, other names it is known by -- trade names and other standards'
designations, UNS numbers among them -- for matching). A printed material names
its process, which matters as much as the material.
"""

from __future__ import annotations

MATERIALS: tuple[tuple[str, str, tuple[str, ...]], ...] = (
    # Aluminium
    ("Aluminium 6082-T6", "Aluminium", ("EN AW-6082", "HE30", "AlSi1MgMn", "A96082")),
    ("Aluminium 6061-T6", "Aluminium", ("EN AW-6061", "AlMg1SiCu", "65032", "A96061")),
    ("Aluminium 6063-T6", "Aluminium", ("EN AW-6063", "HE9", "A96063")),
    ("Aluminium 7075-T6", "Aluminium", ("EN AW-7075", "AlZn5.5MgCu", "aircraft", "A97075")),
    ("Aluminium 7075-T651", "Aluminium", ("EN AW-7075", "plate", "A97075")),
    ("Aluminium 2024-T3", "Aluminium", ("EN AW-2024", "AlCu4Mg1", "duralumin", "A92024")),
    ("Aluminium 2011-T3", "Aluminium", ("EN AW-2011", "free machining", "FC1", "A92011")),
    ("Aluminium 5083-H111", "Aluminium", ("EN AW-5083", "marine", "NS8", "A95083")),
    ("Aluminium 5754-H22", "Aluminium", ("EN AW-5754", "A95754")),
    ("Aluminium 1050A-H14", "Aluminium", ("EN AW-1050A", "pure", "1B", "A91050")),
    ("Aluminium MIC-6 cast plate", "Aluminium", ("tooling plate", "jig plate")),
    ("Aluminium 5052-H32", "Aluminium", ("EN AW-5052", "A95052", "sheet")),
    ("Aluminium 6060-T66", "Aluminium", ("EN AW-6060", "A96060", "extrusion")),
    ("Aluminium 7050-T7451", "Aluminium", ("EN AW-7050", "A97050", "aircraft plate")),
    # Stainless steel
    (
        "Stainless steel 303 (1.4305)",
        "Stainless steel",
        ("X8CrNiS18-9", "free machining", "S30300"),
    ),
    ("Stainless steel 304 (1.4301)", "Stainless steel", ("X5CrNi18-10", "A2", "18/8", "S30400")),
    ("Stainless steel 304L (1.4307)", "Stainless steel", ("X2CrNi18-9", "S30403")),
    (
        "Stainless steel 316 (1.4401)",
        "Stainless steel",
        ("X5CrNiMo17-12-2", "A4", "marine", "S31600"),
    ),
    ("Stainless steel 316L (1.4404)", "Stainless steel", ("X2CrNiMo17-12-2", "A4", "S31603")),
    ("Stainless steel 410 (1.4006)", "Stainless steel", ("X12Cr13", "martensitic", "S41000")),
    ("Stainless steel 416 (1.4005)", "Stainless steel", ("X12CrS13", "free machining", "S41600")),
    ("Stainless steel 420 (1.4021)", "Stainless steel", ("X20Cr13", "martensitic", "S42000")),
    ("Stainless steel 430 (1.4016)", "Stainless steel", ("X6Cr17", "ferritic", "S43000")),
    ("Stainless steel 17-4PH (1.4542)", "Stainless steel", ("630", "X5CrNiCuNb16-4", "S17400")),
    (
        "Stainless steel 2205 duplex (1.4462)",
        "Stainless steel",
        ("X2CrNiMoN22-5-3", "duplex", "S32205"),
    ),
    ("Stainless steel 15-5PH (1.4545)", "Stainless steel", ("S15500", "XM-12")),
    ("Stainless steel 321 (1.4541)", "Stainless steel", ("S32100", "X6CrNiTi18-10")),
    ("Stainless steel 440C (1.4125)", "Stainless steel", ("S44004", "X105CrMo17", "bearing")),
    # Steel
    ("Mild steel S275JR", "Steel", ("43A", "low carbon", "structural")),
    ("Mild steel S355J2", "Steel", ("50D", "structural")),
    ("Steel 1018 (C15)", "Steel", ("080M15", "EN32", "case hardening", "G10180")),
    ("Steel 1020 (C22)", "Steel", ("070M20", "EN3", "bright mild steel", "BMS", "G10200")),
    ("Steel 230M07 free cutting (11SMn30)", "Steel", ("EN1A", "1215", "free cutting", "G12150")),
    ("Steel 230M07Pb leaded free cutting (11SMnPb30)", "Steel", ("EN1A leaded", "12L14", "G12144")),
    ("Steel 1045 (C45)", "Steel", ("080M40", "EN8", "medium carbon", "G10450")),
    ("Steel 080M50 (C50)", "Steel", ("EN43",)),
    ("Steel 4140 (42CrMo4)", "Steel", ("709M40", "EN19", "chromoly", "G41400")),
    ("Steel 4340 (34CrNiMo6)", "Steel", ("817M40", "EN24", "G43400")),
    ("Steel 655M13 (EN36)", "Steel", ("case hardening",)),
    ("Steel 4130 (25CrMo4)", "Steel", ("chromoly", "G41300")),
    ("Spring steel 1095 (C100S)", "Steel", ("EN44", "spring", "G10950")),
    ("Silver steel (115CrV3)", "Steel", ("BS1407", "drill rod")),
    ("Steel 1144 Stressproof", "Steel", ("G11440", "stress proof")),
    ("Steel 605M36 (EN16)", "Steel", ("EN16", "605M36")),
    # Tool steel
    (
        "Tool steel O1 (1.2510)",
        "Tool steel",
        ("100MnCrW4", "oil hardening", "gauge plate", "ground flat stock", "T31501"),
    ),
    ("Tool steel A2 (1.2363)", "Tool steel", ("X100CrMoV5", "air hardening", "T30102")),
    ("Tool steel D2 (1.2379)", "Tool steel", ("X153CrMoV12", "T30402")),
    ("Tool steel H13 (1.2344)", "Tool steel", ("X40CrMoV5-1", "hot work", "T20813")),
    ("Tool steel P20 (1.2311)", "Tool steel", ("40CrMnMo7", "mould steel", "T51620")),
    ("Tool steel S7", "Tool steel", ("shock resisting", "T41907")),
    # Brass and copper
    ("Brass CZ121 (CW614N)", "Brass and copper", ("CuZn39Pb3", "free machining brass", "C38500")),
    ("Brass CZ108 (CW508L)", "Brass and copper", ("CuZn37", "C27200")),
    ("Brass C360", "Brass and copper", ("free cutting brass", "C36000")),
    ("Naval brass CZ112 (CW712R)", "Brass and copper", ("CuZn36Sn1Pb", "C46400")),
    ("Dezincification-resistant brass CZ132 (CW602N)", "Brass and copper", ("DZR", "CuZn36Pb2As")),
    ("Brass CZ120 (CW608N)", "Brass and copper", ("CuZn38Pb2", "leaded brass")),
    ("Lead-free brass CW511L", "Brass and copper", ("CuZn38As", "lead free", "DZR")),
    ("Copper C101 (CW004A)", "Brass and copper", ("Cu-ETP", "C110", "C11000", "electrolytic")),
    ("Oxygen-free copper C103 (CW008A)", "Brass and copper", ("Cu-OF", "C10200", "OFC")),
    ("Phosphor bronze PB102 (CW451K)", "Brass and copper", ("CuSn5", "C51000")),
    ("Bronze SAE 660 (CC493K)", "Brass and copper", ("bearing bronze", "C93200")),
    ("Aluminium bronze CA104 (CW307G)", "Brass and copper", ("CuAl10Ni5Fe4", "C63000")),
    ("Beryllium copper C17200 (CW101C)", "Brass and copper", ("CuBe2", "BeCu", "C17200")),
    # Titanium and others
    (
        "Titanium grade 2 (3.7035)",
        "Titanium and others",
        ("CP titanium", "commercially pure", "R50400"),
    ),
    ("Titanium grade 5 Ti-6Al-4V (3.7165)", "Titanium and others", ("Ti64", "6Al4V", "R56400")),
    ("Inconel 718 (2.4668)", "Titanium and others", ("NiCr19Fe19Nb5Mo3", "nickel alloy", "N07718")),
    ("Inconel 625 (2.4856)", "Titanium and others", ("nickel alloy", "N06625")),
    ("Magnesium AZ31B", "Titanium and others", ("magnesium", "M11311")),
    ("Cast iron EN-GJL-250", "Titanium and others", ("grey iron", "GG25", "grade 250")),
    ("Ductile iron EN-GJS-500-7", "Titanium and others", ("SG iron", "GGG50", "nodular")),
    ("Zinc alloy Zamak 3", "Titanium and others", ("ZP3", "ZnAl4", "die cast zinc")),
    ("Monel 400 (2.4360)", "Titanium and others", ("N04400", "nickel copper")),
    ("Hastelloy C-276 (2.4819)", "Titanium and others", ("N10276", "nickel alloy")),
    ("Tungsten heavy alloy", "Titanium and others", ("Densimet", "tungsten")),
    # Plastics
    (
        "POM-C acetal copolymer",
        "Plastics",
        ("acetal", "Delrin", "Hostaform", "Tecaform", "polyoxymethylene"),
    ),
    ("POM-H acetal homopolymer", "Plastics", ("Delrin", "acetal")),
    ("PA6 nylon", "Plastics", ("nylon 6", "polyamide")),
    ("PA66 nylon", "Plastics", ("nylon 66", "polyamide")),
    ("PA6 cast nylon (oil-filled)", "Plastics", ("Nylatron", "Ertalon")),
    ("PA66 30% glass-filled", "Plastics", ("PA66-GF30", "glass filled nylon")),
    ("PEEK", "Plastics", ("polyetheretherketone", "Ketron")),
    ("PEEK 30% glass-filled", "Plastics", ("PEEK-GF30",)),
    ("PTFE", "Plastics", ("Teflon", "polytetrafluoroethylene")),
    ("UHMW-PE", "Plastics", ("UHMWPE", "PE1000", "polyethylene")),
    ("HDPE", "Plastics", ("PE300", "polyethylene")),
    ("PP polypropylene", "Plastics", ("polypropylene",)),
    ("PC polycarbonate", "Plastics", ("Lexan", "Makrolon", "polycarbonate")),
    ("PMMA acrylic", "Plastics", ("acrylic", "Perspex", "Plexiglas")),
    ("ABS", "Plastics", ("acrylonitrile butadiene styrene",)),
    ("PVC rigid", "Plastics", ("polyvinyl chloride",)),
    ("PET", "Plastics", ("Ertalyte", "polyester")),
    ("PVDF", "Plastics", ("Kynar",)),
    ("PEI", "Plastics", ("Ultem",)),
    ("PPS", "Plastics", ("Techtron", "Ryton")),
    ("Garolite G-10 / FR-4", "Plastics", ("fibreglass laminate", "epoxy glass")),
    ("Tufnol Whale (paper phenolic)", "Plastics", ("phenolic", "Tufnol")),
    ("PSU polysulfone", "Plastics", ("Udel", "polysulphone")),
    ("PPSU", "Plastics", ("Radel", "polyphenylsulfone")),
    ("PBT", "Plastics", ("polybutylene terephthalate",)),
    ("PTFE, glass-filled (Rulon)", "Plastics", ("Rulon", "filled PTFE")),
    ("Polyimide (Vespel)", "Plastics", ("Vespel", "PI")),
    # 3D printing: filament
    ("PLA (FDM)", "3D printing: filament", ("polylactic acid", "printed", "FFF")),
    ("PETG (FDM)", "3D printing: filament", ("PET-G", "printed", "FFF")),
    ("ABS (FDM)", "3D printing: filament", ("printed", "FFF")),
    ("ASA (FDM)", "3D printing: filament", ("UV stable", "printed", "FFF")),
    ("PC (FDM)", "3D printing: filament", ("polycarbonate", "printed", "FFF")),
    ("TPU 95A (FDM)", "3D printing: filament", ("flexible", "TPE", "printed")),
    ("Nylon PA12 (FDM)", "3D printing: filament", ("PA12", "printed", "FFF")),
    ("Carbon-fibre nylon PA12-CF (FDM)", "3D printing: filament", ("CF nylon", "Onyx", "printed")),
    ("PEEK (FDM)", "3D printing: filament", ("printed",)),
    # 3D printing: powder
    ("PA12 (MJF)", "3D printing: powder", ("nylon 12", "multi jet fusion", "HP", "printed")),
    ("PA12 (SLS)", "3D printing: powder", ("nylon 12", "PA2200", "laser sintered", "printed")),
    ("PA11 (MJF)", "3D printing: powder", ("nylon 11", "printed")),
    ("PA12 glass bead filled (MJF)", "3D printing: powder", ("PA12 GB", "glass", "printed")),
    ("TPU (MJF)", "3D printing: powder", ("flexible", "printed")),
    ("PP (MJF)", "3D printing: powder", ("polypropylene", "printed")),
    # 3D printing: resin
    ("Standard resin (SLA)", "3D printing: resin", ("photopolymer", "DLP", "printed")),
    ("Tough resin (SLA)", "3D printing: resin", ("ABS-like", "printed")),
    ("High-temperature resin (SLA)", "3D printing: resin", ("HDT", "printed")),
    ("Flexible resin (SLA)", "3D printing: resin", ("rubber-like", "printed")),
    ("Clear resin (SLA)", "3D printing: resin", ("transparent", "printed")),
    # 3D printing: metal
    ("AlSi10Mg (DMLS)", "3D printing: metal", ("printed aluminium", "SLM", "printed")),
    ("Stainless steel 316L (DMLS)", "3D printing: metal", ("printed stainless", "SLM", "printed")),
    (
        "Stainless steel 17-4PH (DMLS)",
        "3D printing: metal",
        ("printed stainless", "SLM", "printed"),
    ),
    ("Titanium Ti-6Al-4V (DMLS)", "3D printing: metal", ("Ti64", "printed titanium", "printed")),
    ("Inconel 718 (DMLS)", "3D printing: metal", ("printed nickel", "printed")),
    ("Maraging steel MS1 (DMLS)", "3D printing: metal", ("1.2709", "18Ni300", "printed")),
)


def suggestions() -> list[dict[str, object]]:
    """The materials as the material question carries them."""
    return [
        {"value": name, "group": family, "also": list(also)} for name, family, also in MATERIALS
    ]


#: Coatings and surface treatments a shop is asked for, by what they suit.
COATINGS: tuple[tuple[str, str, tuple[str, ...]], ...] = (
    ("None (as machined)", "None", ("bare", "no finish")),
    ("Anodise clear (MIL-A-8625 Type II)", "Aluminium", ("anodize", "natural", "silver")),
    ("Anodise black (MIL-A-8625 Type II)", "Aluminium", ("anodize", "black")),
    ("Hard anodise (MIL-A-8625 Type III)", "Aluminium", ("hardcoat", "anodize")),
    ("Chromate conversion (MIL-DTL-5541)", "Aluminium", ("Alodine", "Iridite", "chem film")),
    ("Zinc plate, clear passivate", "Steel", ("zinc", "BZP", "electroplate")),
    ("Zinc plate, yellow passivate", "Steel", ("zinc", "yellow", "electroplate")),
    ("Black oxide", "Steel", ("blacken", "chemical black", "gun blue")),
    ("Electroless nickel", "Steel", ("ENP", "nickel")),
    ("Hard chrome plate", "Steel", ("chrome", "hard chrome")),
    ("Manganese phosphate", "Steel", ("parkerise", "phosphate")),
    ("Powder coat", "Any", ("paint", "RAL")),
    ("Passivate (ASTM A967)", "Stainless steel", ("passivation",)),
    ("Electropolish", "Stainless steel", ("polish",)),
    ("Bead blast", "Any", ("shot blast", "matte")),
)

#: Heat treatments, by what they suit.
HEAT_TREATMENTS: tuple[tuple[str, str, tuple[str, ...]], ...] = (
    ("None", "None", ("as supplied",)),
    ("Stress relieve", "Steel", ("normalise",)),
    ("Harden and temper to 40–45 HRC", "Steel", ("through harden", "quench", "HRC")),
    ("Harden and temper to 50–55 HRC", "Steel", ("through harden", "quench", "HRC")),
    ("Case harden 0.5–0.8 mm deep, 58–62 HRC", "Steel", ("carburise", "case")),
    ("Nitride", "Steel", ("nitriding", "gas nitride")),
    ("Induction harden", "Steel", ("surface harden",)),
    ("Anneal", "Any", ("soften",)),
    ("Solution treat and age to T6", "Aluminium", ("T6", "precipitation harden")),
)


def suggested(table) -> list[dict[str, object]]:
    """A table of suggestions as a text question carries them."""
    return [{"value": name, "group": group, "also": list(also)} for name, group, also in table]

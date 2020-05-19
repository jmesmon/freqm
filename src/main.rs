#[warn(rust_2018_idioms, missing_debug_implementations, missing_docs)]
use structopt::StructOpt;
use std::path::PathBuf;
use std::convert::TryInto;
use serde::{Serialize, Deserialize};
use snafu::{Snafu, ensure};

#[derive(Debug, StructOpt)]
struct FreqmOpts {
    #[structopt(subcommand)]
    command: FreqmCmd,
}

#[derive(Debug, StructOpt)]
enum FreqmCmd {
    /// list supported radio models with the features we support for them
    Models {

    },
    
    NeCsv { 
        #[structopt(parse(from_os_str))]
        file: PathBuf,
    },
}

#[derive(Serialize, Deserialize, Debug)]
struct NeRepeaterRecord {
    output_freq: String,

    /// '+' => standard offset positive
    /// '-' => standard offset negative
    /// '*' => custom offset, see `notes` field for input freq
    ///
    /// 10 Meters = 100 kHz
    /// 6 Meters = 1 MHz
    /// 2 Meters = 600 kHz
    ///   (Note: 2 Meters also has 1M and 1.5M variants)
    /// 222 MHz = 1.6 MHz
    /// 440 MHz = 5 MHz
    /// 902 MHz = 25 MHz
    /// 1.2 GHz = 12 MHz
    input_offset_dir: String,
    location_state: String,
    location_town: String,

    /// "D-STAR", "DMR", "NFM", "", "NXDN", "P25", "YSF"
    /// "" = Analog 5kHz wide FM
    /// "NFM" = Analog 2.5kHz narrow FM
    ///
    /// often has trailing spaces
    mode: String,

    callsign: String,

    code_in: String,

    /// often omitted for digital modes
    ///
    code_out: String,

    /// "Local", "OFF"
    status: String,

    location_county: String,

    irlp: String,
    echo: String,

    /// common forms:
    ///  - comma seperated
    ///    "field [ ',' field2 ]"
    ///  - fields may be bracketed
    ///    "'['
    ///  - "<freq> (<town>,<state>)": linked repeater
    /// 
    links_and_comments: String,

    /// timestamp := <year> "/" <month> "/" <date>
    /// year := <digit><digit><digit><digit>
    /// month := <digit><digit>
    /// day := <digit><digit>
    /// digit := (0-9)
    update_timestamp: Option<String>,
}

#[derive(Debug, Snafu)]
enum FreqmError {
     #[snafu(display("CSV missing field(s): have {}", field_num))]
    FieldMissing {
        field_num: usize,
    },

    #[snafu(display("output frequency {:?} is not a decimal", output_freq))]
    OutputFreqParseFailure {
        output_freq: String,
    },

    #[snafu(display("offset kind {:?} unrecognized", offset_kind))]
    InvalidOffsetKind {
        offset_kind: String,
    },

    #[snafu(display("frequency {} is not in a known band", freq))]
    FreqNotInAnyBand {
        freq: decimal::d128
    },

    #[snafu(display("comment parse failed: {:?}", comment))]
    CommentParse {
        comment: String,
    }
}

impl std::convert::TryFrom<csv::StringRecord> for NeRepeaterRecord {
    type Error = Box<dyn std::error::Error>;
    fn try_from(s: csv::StringRecord) -> Result<Self, Self::Error> {
        // variations:
        //  - update time stamp omitted
        //  - trailing comma omitted
        //
        // NOTE: extra field is because each line has a trailing comma (csv parses as an empty
        // field
        ensure!(
            s.len() == 13 || s.len() == 14 || s.len() == 15,
            FieldMissing { field_num: s.len()}
        );

        Ok(Self {
            output_freq: s.get(0).unwrap().to_owned(),
            input_offset_dir: s.get(1).unwrap().to_owned(),
            location_state: s.get(2).unwrap().to_owned(),
            location_town: s.get(3).unwrap().to_owned(),

            mode: s.get(4).unwrap().to_owned(),
            callsign: s.get(5).unwrap().to_owned(),
            code_in: s.get(6).unwrap().to_owned(),
            code_out: s.get(7).unwrap().to_owned(),

            status: s.get(8).unwrap().to_owned(),

            location_county: s.get(9).unwrap().to_owned(),
            irlp: s.get(10).unwrap().to_owned(),
            echo: s.get(11).unwrap().to_owned(),
            links_and_comments: s.get(12).unwrap().to_owned(),
            update_timestamp: s.get(13).map(|x| x.to_owned()),
        })
    }
}


/// Given a frequency in the New England region, calculate it's standard offset
/// 
/// 10 Meters = 100 kHz
/// 6 Meters = 1 MHz
/// 2 Meters = 600 kHz
///   (Note: 2 Meters also has 1M and 1.5M variants)
/// 222 MHz = 1.6 MHz
/// 440 MHz = 5 MHz
/// 902 MHz = 25 MHz
/// 1.2 GHz = 12 MHz
fn standard_offset_new_england(mhz: decimal::d128) -> Option<decimal::d128> {
    if mhz >= 28.into() && mhz <= decimal::d128!(29.7) {
        Some(decimal::d128!(0.100))
    } else if mhz >= 50.into() && mhz <= 54.into() {
        Some(decimal::d128!(1))
    } else if mhz >= 144.into() && mhz <= 148.into() {
        Some(decimal::d128!(0.600))
    } else if mhz >= 219.into() && mhz <= 225.into() {
        Some(decimal::d128!(1.6))
    } else if mhz >= 420.into() && mhz <= 450.into() {
        Some(decimal::d128!(5))
    } else if mhz >= 902.into() && mhz <= 928.into() {
        Some(decimal::d128!(25))
    } else if mhz >= 1240.into() && mhz <= 1300.into() {
        Some(decimal::d128!(12))
    } else {
        None
    }
}

#[derive(Debug)]
pub struct Repeater {
    output_freq: decimal::d128,

    /// optional because some datasets don't include this
    input_freq: Option<decimal::d128>,
}

impl std::convert::TryFrom<NeRepeaterRecord> for Repeater {
    type Error = FreqmError;
    fn try_from(nerr: NeRepeaterRecord) -> Result<Self, Self::Error> {
        let output_freq: decimal::d128 = nerr.output_freq.parse()
            .map_err(|_| FreqmError::OutputFreqParseFailure { output_freq: nerr.output_freq.clone() })?;

        let input_freq = match &nerr.input_offset_dir[..] {
            "+" => {
                let shift = standard_offset_new_england(output_freq)
                    .ok_or(FreqmError::FreqNotInAnyBand { freq: output_freq })?;
                Some(output_freq + shift)
            },
            "-" => {
                let shift = standard_offset_new_england(output_freq)
                    .ok_or(FreqmError::FreqNotInAnyBand { freq: output_freq })?;
                Some(output_freq - shift)
            },
            "*" => {
                // examine the `note` field
                let prefix = "*Input: ";
                if nerr.links_and_comments.starts_with(prefix) {
                    // remove the prefix, take the first number as the frequency in MHz
                    let s = &nerr.links_and_comments[prefix.len()..];
                    let mut si = s.split(" ");
                    let n = si.next().ok_or(FreqmError::CommentParse { comment: nerr.links_and_comments.clone() })?;

                    Some(n.parse().map_err(|_| FreqmError::CommentParse { comment: nerr.links_and_comments.clone() })?)
                } else {
                    // some entries indicate a special split but
                    None
                }
            },
            "S" => {
                // simplex?
                Some(output_freq)
            },
            other => {
                return InvalidOffsetKind { offset_kind: nerr.input_offset_dir.clone() }.fail()?;
            }
        };

        Ok(Self {
            output_freq,
            input_freq
        })
    }
}

impl Repeater {
    fn freq_as_offset(&self) -> (f64, f64) {
        todo!()
    }

    fn bandwidth(&self) -> f64 {
        todo!()
    }

    fn mode(&self) -> usize {
        todo!()
    }

    /// Note: codes are limited by `mode`, consider if we should have a `mode` which contains the
    /// code info
    fn code_in(&self) -> Option<usize> {
        todo!()
    }

    fn code_out(&self) -> Option<usize> {
        todo!()
    }

}

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let opt = FreqmOpts::from_args();

    match opt.command {
        FreqmCmd::NeCsv { file } => {
            let mut csv = csv::ReaderBuilder::new()
                .has_headers(false)
                .flexible(true)
                .from_path(file)?;

            for r in csv.records() {
                let r = r?;
                println!("{:?}", r);
                let record: NeRepeaterRecord = r.try_into()?;

                println!("{:?}", record); 

                let r2: Repeater = record.try_into()?;

                println!("{:?}", r2);

            }
        },
        FreqmCmd::Models { } => {
            todo!("list-models");
        }
    }

    Ok(())
}

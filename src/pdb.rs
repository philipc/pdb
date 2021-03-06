// Copyright 2017 pdb Developers
//
// Licensed under the Apache License, Version 2.0, <LICENSE-APACHE or
// http://apache.org/licenses/LICENSE-2.0> or the MIT license <LICENSE-MIT or
// http://opensource.org/licenses/MIT>, at your option. This file may not be
// copied, modified, or distributed except according to those terms.

use dbi;
use msf;
use symbol;
use tpi;

use common::*;
use dbi::DebugInformation;
use source::Source;
use msf::{MSF, Stream};
use symbol::SymbolTable;
use tpi::TypeInformation;

/// `PDB` provides access to the data within a PDB file.
///
/// A PDB file is internally a Multi-Stream File (MSF), composed of multiple independent
/// (and usually discontiguous) data streams on-disk. `PDB` provides lazy access to these data
/// structures, which means the `PDB` accessor methods usually cause disk accesses.
#[derive(Debug)]
pub struct PDB<'s, S> {
    /// `msf` provides access to the underlying data streams
    msf: Box<MSF<'s, S> + 's>,

    /// Memoize the `dbi::Header`, since it contains stream numbers we sometimes need
    dbi_header: Option<dbi::Header>,
}

impl<'s, S: Source<'s> + 's> PDB<'s, S> {
    /// Create a new `PDB` for a `Source`.
    ///
    /// `open()` accesses enough of the source file to find the MSF stream table. This usually
    /// involves reading the header, a block near the end of the file, and finally the stream table
    /// itself. It does not access or validate any of the contents of the rest of the PDB.
    ///
    /// # Errors
    ///
    /// * `Error::UnimplementedFeature` if the PDB file predates ~2002
    /// * `Error::UnrecognizedFileFormat` if the `Source` does not appear to be a PDB file
    /// * `Error::IoError` if returned by the `Source`
    /// * `Error::PageReferenceOutOfRange`, `Error::InvalidPageSize` if the PDB file seems corrupt
    pub fn open(source: S) -> Result<PDB<'s, S>> {
        let msf = msf::open_msf(source)?;

        Ok(PDB{
            msf: msf,
            dbi_header: None,
        })
    }

    /// Retrieve the `TypeInformation` for this PDB.
    ///
    /// The `TypeInformation` object owns a `SourceView` for the type information ("TPI") stream.
    /// This is usually the single largest stream of the PDB file.
    ///
    /// # Errors
    ///
    /// * `Error::StreamNotFound` if the PDB somehow does not contain the type information stream
    /// * `Error::IoError` if returned by the `Source`
    /// * `Error::PageReferenceOutOfRange` if the PDB file seems corrupt
    /// * `Error::InvalidTypeInformationHeader` if the type information stream header was not
    ///   understood
    pub fn type_information(&mut self) -> Result<TypeInformation<'s>> {
        // The TPI stream is always stream number 2:
        //   https://github.com/Microsoft/microsoft-pdb/blob/082c5290e5aff028ae84e43affa8be717aa7af73/PDB/dbi/dbiimpl.h#L67
        // Open that stream
        let stream: Stream = self.msf.get(2, None)?;

        // Parse it
        let type_info = tpi::new_type_information(stream)?;

        // Return
        return Ok(type_info);
    }

    /// Retrieve the `DebugInformation` for this PDB.
    ///
    /// The `DebugInformation` object owns a `SourceView` for the debug information ("DBI") stream.
    ///
    /// # Errors
    ///
    /// * `Error::StreamNotFound` if the PDB somehow does not contain a symbol records stream
    /// * `Error::IoError` if returned by the `Source`
    /// * `Error::PageReferenceOutOfRange` if the PDB file seems corrupt
    /// * `Error::UnimplementedFeature` if the debug information header predates ~1995
    pub fn debug_information(&mut self) -> Result<DebugInformation<'s>> {
        // The DBI stream is always stream number 3:
        //   https://github.com/Microsoft/microsoft-pdb/blob/082c5290e5aff028ae84e43affa8be717aa7af73/PDB/dbi/dbiimpl.h#L68
        // Open that stream
        let stream: Stream = self.msf.get(3, None)?;

        // Parse it
        let debug_info = dbi::new_debug_information(stream)?;

        // Grab its header, since we need that for unrelated operations
        self.dbi_header = Some(dbi::get_header(&debug_info));

        // Return
        return Ok(debug_info);
    }

    fn dbi_header(&mut self) -> Result<dbi::Header> {
        // see if we've already got a header
        if let Some(ref h) = self.dbi_header {
            return Ok(*h);
        }

        let header;

        {
            // get just the first little bit of stream 3
            let stream: Stream = self.msf.get(3, Some(1024))?;
            let mut buf = stream.parse_buffer();
            header = dbi::parse_header(&mut buf)?;
        }

        self.dbi_header = Some(header);

        Ok(header)
    }

    /// Retrieve the global symbol table for this PDB.
    ///
    /// The `SymbolTable` object owns a `SourceView` for the symbol records stream. This is usually
    /// the second-largest stream of the PDB file.
    ///
    /// The debug information stream indicates which stream is the symbol records stream, so
    /// `global_symbols()` accesses the debug information stream to read the header unless
    /// `debug_information()` was called first.
    ///
    /// # Errors
    ///
    /// * `Error::StreamNotFound` if the PDB somehow does not contain a symbol records stream
    /// * `Error::IoError` if returned by the `Source`
    /// * `Error::PageReferenceOutOfRange` if the PDB file seems corrupt
    ///
    /// If `debug_information()` was not already called, `global_symbols()` will additionally read
    /// the debug information header, in which case it can also return:
    ///
    /// * `Error::StreamNotFound` if the PDB somehow does not contain a debug information stream
    /// * `Error::UnimplementedFeature` if the debug information header predates ~1995
    pub fn global_symbols(&mut self) -> Result<SymbolTable<'s>> {
        // the global symbol table is stored in a stream number described by the DBI header
        // so, start by getting the DBI header
        let dbi_header = self.dbi_header()?;

        // open the appropriate stream
        let stream: Stream = self.msf.get(dbi_header.symbol_records_stream as u32, None)?;

        return Ok(symbol::new_symbol_table(stream));
    }
}

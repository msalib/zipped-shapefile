use std::{
    fs::File,
    io::{Cursor, Read, Seek},
    path::Path,
};

pub use shapefile::{dbase::FieldValue, reader::ShapeRecordIterator, Reader, Shape, ShapeReader};
use thiserror::Error;
use zip::ZipArchive;

// FIXME: optional geo-types feature

#[derive(Error, Debug)]
pub enum Error {
    #[error("IO Error")]
    IOError(#[from] std::io::Error),

    #[error("DBase Error")]
    DBase(#[from] dbase::Error),

    #[error("Zipfile Error")]
    Zip(#[from] zip::result::ZipError),

    #[error("Shapefile Error")]
    Shapefile(#[from] shapefile::Error),

    #[error("Multiple files found with extension {0}")]
    MultipleFilesFound(&'static str),

    #[error("No .shp file found in zipfile")]
    NoShpFound,

    #[error("unknown")]
    Unknown,

    #[error("zip member size larger than `usize`")]
    MemberSizeTooLarge(u64),

    #[error("No .dfb file found in zipfile")]
    NoDbfFound,
}

pub type Result<T> = std::result::Result<T, Error>;

pub struct ZippedShapefile<R> {
    archive: ZipArchive<R>,
    projection: Option<String>,
    shp: String,
    shx: Option<String>,
    dbf: Option<String>,
}

impl ZippedShapefile<std::fs::File> {
    pub fn open<P>(path: P) -> Result<Self>
    where
        P: AsRef<Path>,
    {
        ZippedShapefile::new(File::open(path)?)
    }
}

impl<R> ZippedShapefile<R>
where
    R: Read + Seek,
{
    pub fn new(source: R) -> Result<Self> {
        let mut archive = ZipArchive::new(source)?;
        let mut shp = None;
        let mut shx = None;
        let mut dbf = None;
        let mut prj = None;

        for member in archive.file_names() {
            if member.ends_with(".shp") {
                if shp.is_some() {
                    return Err(Error::MultipleFilesFound(".shp"));
                }
                shp = Some(member.to_owned());
            } else if member.ends_with(".shx") {
                if shx.is_some() {
                    return Err(Error::MultipleFilesFound(".shx"));
                }
                shx = Some(member.to_owned())
            } else if member.ends_with(".dbf") {
                if dbf.is_some() {
                    return Err(Error::MultipleFilesFound(".dbf"));
                }
                dbf = Some(member.to_owned())
            } else if member.ends_with(".prj") {
                if prj.is_some() {
                    return Err(Error::MultipleFilesFound(".prj"));
                }
                prj = Some(member.to_owned());
            }
        }

        let projection = if let Some(prj) = prj {
            let mut wkt = String::new();
            let mut wkt_buf = archive.by_name(&prj)?;
            wkt_buf.read_to_string(&mut wkt)?;
            Some(wkt)
        } else {
            None
        };

        match shp {
            Some(shp) => Ok(Self {
                archive,
                projection,
                shp,
                shx,
                dbf,
            }),
            None => Err(Error::NoShpFound),
        }
    }

    fn read_member(&mut self, name: &str) -> Result<Cursor<Vec<u8>>> {
        let mut zf = self.archive.by_name(name)?;
        let size: usize = zf
            .size()
            .try_into()
            .map_err(|_| Error::MemberSizeTooLarge(zf.size()))?;
        let mut buf = Vec::with_capacity(size);
        assert_eq!(size, zf.read_to_end(&mut buf)?);
        Ok(Cursor::new(buf))
    }

    pub fn projection(&self) -> Option<&str> {
        self.projection.as_deref()
    }

    pub fn shape_reader(&mut self) -> Result<ShapeReader<Cursor<Vec<u8>>>> {
        let shp = self.shp.clone();
        let shx = self.shx.clone();
        let shp_reader = self.read_member(&shp)?;
        Ok(if let Some(shx) = &shx {
            ShapeReader::with_shx(shp_reader, self.read_member(shx)?)
        } else {
            ShapeReader::new(shp_reader)
        }?)
    }

    pub fn dbf_reader(&mut self) -> Result<Option<dbase::Reader<Cursor<Vec<u8>>>>> {
        match self.dbf.clone() {
            Some(dbf) => Ok(Some(dbase::Reader::new(self.read_member(&dbf)?)?)),
            None => Ok(None),
        }
    }

    pub fn reader(&mut self) -> Result<Reader<Cursor<Vec<u8>>>> {
        let dbf = self
            .dbf_reader()
            .transpose()
            .unwrap_or(Err(Error::NoDbfFound))?;
        let shp = self.shape_reader()?;
        Ok(Reader::new(shp, dbf))
    }

    pub fn types(&mut self) -> Result<Option<Vec<(String, String)>>> {
        // `field_type` is private alas but we can still turn it into a string
        Ok(self.dbf_reader()?.map(|reader| {
            reader
                .fields()
                .iter()
                .map(|field| (field.name().to_owned(), field.field_type().to_string()))
                .collect()
        }))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        let result = 2 + 2;
        assert_eq!(result, 4);
    }
}

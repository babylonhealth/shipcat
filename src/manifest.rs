use serde_yaml;

use std::io::prelude::*;
use std::fs::File;
use std::env;
use std::path::{PathBuf, Path};
use std::collections::BTreeMap;

use super::BabylResult;

// k8s related structs

#[derive(Serialize, Deserialize, Clone)]
pub struct ResourceRequest {
    /// CPU request string
    cpu: String,
    /// Memory request string
    memory: String,
    // TODO: ephemeral-storage + extended-resources
}

#[derive(Serialize, Deserialize, Clone)]
pub struct ResourceLimit {
    /// CPU limit string
    cpu: String,
    /// Memory limit string
    memory: String,
    // TODO: ephemeral-storage + extended-resources
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Resources {
    /// Resource requests for k8s
    pub requests: Option<ResourceRequest>,
    /// Resource limits for k8s
    pub limits: Option<ResourceLimit>,
}


#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Replicas {
    /// Minimum replicas for k8s deployment
    pub min: u32,
    /// Maximum replicas for k8s deployment
    pub max: u32,
}


#[derive(Serialize, Deserialize, Clone, Default)]
pub struct ConfigMount {
    /// Name of file to mount as in k8s
    pub name: String,
    /// Local location of template
    pub src: String,
}


// misc structs

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Dashboard {
    /// Metric strings to track
    pub rows: Vec<String>,
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Prometheus {
    /// Whether to poll
    pub enabled: bool,
    /// Path to poll
    pub path: String,
    // TODO: Maybe include names of metrics?
}


/// Main manifest, serializable from babyl.yaml
#[derive(Serialize, Deserialize, Clone, Default)]
pub struct Manifest {
    /// Name of the main component
    pub name: String,

    // Kubernetes specific flags

    /// Resource limits and requests
    #[serde(skip_serializing_if = "Option::is_none")]
    pub resources: Option<Resources>,
    /// Replication limits
    #[serde(skip_serializing_if = "Option::is_none")]
    pub replicas: Option<Replicas>,
    /// Environment variables to inject
    #[serde(skip_serializing_if = "Option::is_none")]
    pub env: Option<BTreeMap<String, String>>,
    /// Environment file to mount
    #[serde(skip_serializing_if = "Option::is_none")]
    pub config: Option<ConfigMount>,


    /// Prometheus metric options
    #[serde(skip_serializing_if = "Option::is_none")]
    pub prometheus: Option<Prometheus>,
//prometheus:
//  enabled: true
//  path: /metrics
    /// Dashboards to generate
    #[serde(skip_serializing_if = "BTreeMap::is_empty")]
    pub dashboards: BTreeMap<String, Dashboard>,
//dashboards:
//  auth-python:
//    rows:
//      - users-connected
//      - conversation-length

    // TODO: health/die, secrets/vault, logging alerts
//vault:
//  path: /blah/woot
//logging:
//  alerts:
//    error-rate-5xx:
//      type: median
//      threshold: 2
//      status-code: 500
//preStopHookPath: /die
    // TODO: newrelic token
//newrelic:
//  license: arsitnwf234430iearosnt

    // Internal path of this manifest
    #[serde(skip_serializing, skip_deserializing)]
    location: String,
}


impl Manifest {
    pub fn new(name: &str, location: PathBuf) -> Manifest {
        Manifest {
            name: name.into(),
            location: location.to_string_lossy().into(),
            ..Default::default()
        }
    }
    /// Read a manifest file in an arbitrary path
    pub fn read_from(pwd: &PathBuf) -> BabylResult<Manifest> {
        let mpath = pwd.join("babyl.yaml");
        trace!("Using manifest in {}", mpath.display());
        let mut f = File::open(&mpath)?;
        let mut data = String::new();
        f.read_to_string(&mut data)?;
        let mut res: Manifest = serde_yaml::from_str(&data)?;
        // store the location internally (not serialized to disk)
        res.location = mpath.to_string_lossy().into();
        Ok(res)
    }

    /// Read a manifest file in PWD
    pub fn read() -> BabylResult<Manifest> {
        Ok(Manifest::read_from(&Path::new(".").to_path_buf())?)
    }

    /// Fills in defaults from config file
    pub fn fill(&mut self) -> BabylResult<()> {
        // TODO: put the defaults file somewhere!
        let data = "name: default-service\
resources:\
  limits:\
    cpu: 800m\
    memory: 1024Mi\
  requests:\
    cpu: 200m\
    memory: 512Mi\
replicas:\
  min: 2\
  max: 4\
";
        let mf: Manifest = serde_yaml::from_str(&data)?;

        if self.resources.is_none() {
            self.resources = mf.resources.clone();
        }
        if let Some(ref mut res) = self.resources {
            if res.limits.is_none() {
                res.limits = mf.resources.clone().unwrap().limits;
            }
            if res.requests.is_none() {
                res.requests = mf.resources.clone().unwrap().requests;
            }
            // for now: if limits or requests are specified, you have to fill in both CPU and memory
        }
        if self.replicas.is_none() {
            self.replicas = mf.replicas;
        }
        Ok(())
    }

    /// Update the manifest file in the current folder
    pub fn write(&self) -> BabylResult<()> {
        let encoded = serde_yaml::to_string(self)?;
        trace!("Writing manifest in {}", self.location);
        let mut f = File::create(&self.location)?;
        write!(f, "{}\n", encoded)?;
        debug!("Wrote manifest in {}: \n{}", self.location, encoded);
        Ok(())
    }

    /// Verify assumptions about manifest
    ///
    /// Assumes the manifest has been `fill()`ed.
    pub fn verify(&self) -> BabylResult<()> {
        if self.name == "" {
            bail!("Name cannot be empty")
        }
        // 1. Verify resources
        let req = self.resources.clone().unwrap().requests.unwrap().clone();
        let lim = self.resources.clone().unwrap().limits.unwrap().clone();
        let req_memory = parse_memory(&req.memory)?;
        let lim_memory = parse_memory(&lim.memory)?;
        let req_cpu = parse_cpu(&req.cpu)?;
        let lim_cpu = parse_cpu(&lim.cpu)?;

        // 1.1 limits >= requests
        if req_cpu > lim_cpu {
            bail!("Requested more CPU than what was limited");
        }
        if req_memory > lim_memory {
            bail!("Requested more memory than what was limited");
        }
        // 1.2 sanity numbers
        if req_cpu > 10.0 {
            bail!("Requested more than 10 cores");
        }
        if req_memory > 10*1024*1024*1024 {
            bail!("Requested more than 10 GB of memory");
        }

        // 2. TODO: other keys

        Ok(())
    }

}

// Parse normal k8s memory resource value into integers
fn parse_memory(s: &str) -> BabylResult<u64> {
    let digits = s.chars().take_while(|ch| ch.is_digit(10)).collect::<String>();
    let unit = s.chars().skip_while(|ch| ch.is_digit(10)).collect::<String>();
    let mut res : u64 = digits.parse()?;
    trace!("Parsed {} ({})", digits, unit);
    if unit == "Ki" {
        res *= 1024;
    } else if unit == "Mi" {
        res *= 1024*1024;
    } else if unit == "Gi" {
        res *= 1024*1024*1024;
    } else if unit == "k" {
        res *= 1000;
    } else if unit == "M" {
        res *= 1000*1000;
    } else if unit == "G" {
        res *= 1000*1000*1000;
    } else if unit != "" {
        bail!("Unknown unit {}", unit);
    }
    trace!("Returned {} bytes", res);
    Ok(res)
}

// Parse normal k8s cpu resource values into floats
// We don't allow power of two variants here
fn parse_cpu(s: &str) -> BabylResult<f64> {
    let digits = s.chars().take_while(|ch| ch.is_digit(10)).collect::<String>();
    let unit = s.chars().skip_while(|ch| ch.is_digit(10)).collect::<String>();
    let mut res : f64 = digits.parse()?;

    trace!("Parsed {} ({})", digits, unit);
    if unit == "m" {
        res /= 1000.0;
    } else if unit == "k" {
        res *= 1000.0;
    } else if unit != "" {
        bail!("Unknown unit {}", unit);
    }
    trace!("Returned {} cores", res);
    Ok(res)
}

pub fn validate() -> BabylResult<()> {
    let mut mf = Manifest::read()?;
    mf.fill()?;
    mf.verify()
}

pub fn init() -> BabylResult<()> {
    let pwd = env::current_dir()?;
    let last_comp = pwd.components().last().unwrap(); // std::path::Component
    let dirname = last_comp.as_os_str().to_str().unwrap();

    let mf = Manifest::new(dirname, pwd.join("babyl.yaml"));
    mf.write()
}

use crate::cloud_provider::kubernetes::InstanceType;
use crate::errors::CommandError;
use core::fmt;
use serde::{Deserialize, Serialize};
use std::str::FromStr;
use strum_macros::EnumIter;

/// DO NOT MANUALLY EDIT THIS FILE. IT IS AUTO-GENERATED BY INSTANCES FETCHER APP
/// https://gitlab.com/qovery/backend/rust-backend/instances-fetcher/src/lib/rust_generator.rs
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, EnumIter)]
#[allow(non_camel_case_types)]
pub enum ScwInstancesType {
    DEV1_L,
    DEV1_M,
    DEV1_S,
    DEV1_XL,
    ENT1_L,
    ENT1_M,
    ENT1_S,
    ENT1_XS,
    ENT1_XXS,
    GP1_L,
    GP1_M,
    GP1_S,
    GP1_VIZ,
    GP1_XS,
    GPU_3070_S,
    PLAY2_MICRO,
    PLAY2_NANO,
    PLAY2_PICO,
    PRO2_L,
    PRO2_M,
    PRO2_S,
    PRO2_XS,
    PRO2_XXS,
    RENDER_S,
    START1_L,
    START1_M,
    START1_S,
    VC1L,
    VC1M,
    VC1S,
    X64_120GB,
    X64_15GB,
    X64_30GB,
    X64_60GB,
}

impl InstanceType for ScwInstancesType {
    fn to_cloud_provider_format(&self) -> String {
        match self {
            ScwInstancesType::DEV1_L => "dev1-l",
            ScwInstancesType::DEV1_M => "dev1-m",
            ScwInstancesType::DEV1_S => "dev1-s",
            ScwInstancesType::DEV1_XL => "dev1-xl",
            ScwInstancesType::ENT1_L => "ent1-l",
            ScwInstancesType::ENT1_M => "ent1-m",
            ScwInstancesType::ENT1_S => "ent1-s",
            ScwInstancesType::ENT1_XS => "ent1-xs",
            ScwInstancesType::ENT1_XXS => "ent1-xxs",
            ScwInstancesType::GP1_L => "gp1-l",
            ScwInstancesType::GP1_M => "gp1-m",
            ScwInstancesType::GP1_S => "gp1-s",
            ScwInstancesType::GP1_VIZ => "gp1-viz",
            ScwInstancesType::GP1_XS => "gp1-xs",
            ScwInstancesType::GPU_3070_S => "gpu-3070-s",
            ScwInstancesType::PLAY2_MICRO => "play2-micro",
            ScwInstancesType::PLAY2_NANO => "play2-nano",
            ScwInstancesType::PLAY2_PICO => "play2-pico",
            ScwInstancesType::PRO2_L => "pro2-l",
            ScwInstancesType::PRO2_M => "pro2-m",
            ScwInstancesType::PRO2_S => "pro2-s",
            ScwInstancesType::PRO2_XS => "pro2-xs",
            ScwInstancesType::PRO2_XXS => "pro2-xxs",
            ScwInstancesType::RENDER_S => "render-s",
            ScwInstancesType::START1_L => "start1-l",
            ScwInstancesType::START1_M => "start1-m",
            ScwInstancesType::START1_S => "start1-s",
            ScwInstancesType::VC1L => "vc1l",
            ScwInstancesType::VC1M => "vc1m",
            ScwInstancesType::VC1S => "vc1s",
            ScwInstancesType::X64_120GB => "x64-120gb",
            ScwInstancesType::X64_15GB => "x64-15gb",
            ScwInstancesType::X64_30GB => "x64-30gb",
            ScwInstancesType::X64_60GB => "x64-60gb",
        }
        .to_string()
    }

    fn is_instance_allowed(&self) -> bool {
        matches!(
            self,
            ScwInstancesType::DEV1_L
                | ScwInstancesType::DEV1_M
                | ScwInstancesType::DEV1_S
                | ScwInstancesType::DEV1_XL
                | ScwInstancesType::ENT1_L
                | ScwInstancesType::ENT1_M
                | ScwInstancesType::ENT1_S
                | ScwInstancesType::ENT1_XS
                | ScwInstancesType::ENT1_XXS
                | ScwInstancesType::GP1_L
                | ScwInstancesType::GP1_M
                | ScwInstancesType::GP1_S
                | ScwInstancesType::GP1_VIZ
                | ScwInstancesType::GP1_XS
                | ScwInstancesType::GPU_3070_S
                | ScwInstancesType::PLAY2_MICRO
                | ScwInstancesType::PLAY2_NANO
                | ScwInstancesType::PLAY2_PICO
                | ScwInstancesType::PRO2_L
                | ScwInstancesType::PRO2_M
                | ScwInstancesType::PRO2_S
                | ScwInstancesType::PRO2_XS
                | ScwInstancesType::PRO2_XXS
                | ScwInstancesType::RENDER_S
                | ScwInstancesType::START1_L
                | ScwInstancesType::START1_M
                | ScwInstancesType::START1_S
                | ScwInstancesType::VC1L
                | ScwInstancesType::VC1M
                | ScwInstancesType::VC1S
                | ScwInstancesType::X64_120GB
                | ScwInstancesType::X64_15GB
                | ScwInstancesType::X64_30GB
                | ScwInstancesType::X64_60GB
        )
    }
    fn is_arm_instance(&self) -> bool {
        false
    }

    fn is_instance_cluster_allowed(&self) -> bool {
        matches!(
            self,
            ScwInstancesType::DEV1_L
                | ScwInstancesType::DEV1_M
                | ScwInstancesType::DEV1_XL
                | ScwInstancesType::ENT1_L
                | ScwInstancesType::ENT1_M
                | ScwInstancesType::ENT1_S
                | ScwInstancesType::ENT1_XS
                | ScwInstancesType::ENT1_XXS
                | ScwInstancesType::GP1_L
                | ScwInstancesType::GP1_M
                | ScwInstancesType::GP1_S
                | ScwInstancesType::GP1_VIZ
                | ScwInstancesType::GP1_XS
                | ScwInstancesType::GPU_3070_S
                | ScwInstancesType::PLAY2_MICRO
                | ScwInstancesType::PLAY2_NANO
                | ScwInstancesType::PRO2_L
                | ScwInstancesType::PRO2_M
                | ScwInstancesType::PRO2_S
                | ScwInstancesType::PRO2_XS
                | ScwInstancesType::PRO2_XXS
                | ScwInstancesType::RENDER_S
                | ScwInstancesType::START1_L
                | ScwInstancesType::START1_M
                | ScwInstancesType::VC1L
                | ScwInstancesType::VC1M
                | ScwInstancesType::X64_120GB
                | ScwInstancesType::X64_15GB
                | ScwInstancesType::X64_30GB
                | ScwInstancesType::X64_60GB
        )
    }
}

impl ScwInstancesType {
    pub fn as_str(&self) -> &str {
        match self {
            ScwInstancesType::DEV1_L => "dev1-l",
            ScwInstancesType::DEV1_M => "dev1-m",
            ScwInstancesType::DEV1_S => "dev1-s",
            ScwInstancesType::DEV1_XL => "dev1-xl",
            ScwInstancesType::ENT1_L => "ent1-l",
            ScwInstancesType::ENT1_M => "ent1-m",
            ScwInstancesType::ENT1_S => "ent1-s",
            ScwInstancesType::ENT1_XS => "ent1-xs",
            ScwInstancesType::ENT1_XXS => "ent1-xxs",
            ScwInstancesType::GP1_L => "gp1-l",
            ScwInstancesType::GP1_M => "gp1-m",
            ScwInstancesType::GP1_S => "gp1-s",
            ScwInstancesType::GP1_VIZ => "gp1-viz",
            ScwInstancesType::GP1_XS => "gp1-xs",
            ScwInstancesType::GPU_3070_S => "gpu-3070-s",
            ScwInstancesType::PLAY2_MICRO => "play2-micro",
            ScwInstancesType::PLAY2_NANO => "play2-nano",
            ScwInstancesType::PLAY2_PICO => "play2-pico",
            ScwInstancesType::PRO2_L => "pro2-l",
            ScwInstancesType::PRO2_M => "pro2-m",
            ScwInstancesType::PRO2_S => "pro2-s",
            ScwInstancesType::PRO2_XS => "pro2-xs",
            ScwInstancesType::PRO2_XXS => "pro2-xxs",
            ScwInstancesType::RENDER_S => "render-s",
            ScwInstancesType::START1_L => "start1-l",
            ScwInstancesType::START1_M => "start1-m",
            ScwInstancesType::START1_S => "start1-s",
            ScwInstancesType::VC1L => "vc1l",
            ScwInstancesType::VC1M => "vc1m",
            ScwInstancesType::VC1S => "vc1s",
            ScwInstancesType::X64_120GB => "x64-120gb",
            ScwInstancesType::X64_15GB => "x64-15gb",
            ScwInstancesType::X64_30GB => "x64-30gb",
            ScwInstancesType::X64_60GB => "x64-60gb",
        }
    }
}

impl fmt::Display for ScwInstancesType {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match self {
            ScwInstancesType::DEV1_L => write!(f, "dev1-l"),
            ScwInstancesType::DEV1_M => write!(f, "dev1-m"),
            ScwInstancesType::DEV1_S => write!(f, "dev1-s"),
            ScwInstancesType::DEV1_XL => write!(f, "dev1-xl"),
            ScwInstancesType::ENT1_L => write!(f, "ent1-l"),
            ScwInstancesType::ENT1_M => write!(f, "ent1-m"),
            ScwInstancesType::ENT1_S => write!(f, "ent1-s"),
            ScwInstancesType::ENT1_XS => write!(f, "ent1-xs"),
            ScwInstancesType::ENT1_XXS => write!(f, "ent1-xxs"),
            ScwInstancesType::GP1_L => write!(f, "gp1-l"),
            ScwInstancesType::GP1_M => write!(f, "gp1-m"),
            ScwInstancesType::GP1_S => write!(f, "gp1-s"),
            ScwInstancesType::GP1_VIZ => write!(f, "gp1-viz"),
            ScwInstancesType::GP1_XS => write!(f, "gp1-xs"),
            ScwInstancesType::GPU_3070_S => write!(f, "gpu-3070-s"),
            ScwInstancesType::PLAY2_MICRO => write!(f, "play2-micro"),
            ScwInstancesType::PLAY2_NANO => write!(f, "play2-nano"),
            ScwInstancesType::PLAY2_PICO => write!(f, "play2-pico"),
            ScwInstancesType::PRO2_L => write!(f, "pro2-l"),
            ScwInstancesType::PRO2_M => write!(f, "pro2-m"),
            ScwInstancesType::PRO2_S => write!(f, "pro2-s"),
            ScwInstancesType::PRO2_XS => write!(f, "pro2-xs"),
            ScwInstancesType::PRO2_XXS => write!(f, "pro2-xxs"),
            ScwInstancesType::RENDER_S => write!(f, "render-s"),
            ScwInstancesType::START1_L => write!(f, "start1-l"),
            ScwInstancesType::START1_M => write!(f, "start1-m"),
            ScwInstancesType::START1_S => write!(f, "start1-s"),
            ScwInstancesType::VC1L => write!(f, "vc1l"),
            ScwInstancesType::VC1M => write!(f, "vc1m"),
            ScwInstancesType::VC1S => write!(f, "vc1s"),
            ScwInstancesType::X64_120GB => write!(f, "x64-120gb"),
            ScwInstancesType::X64_15GB => write!(f, "x64-15gb"),
            ScwInstancesType::X64_30GB => write!(f, "x64-30gb"),
            ScwInstancesType::X64_60GB => write!(f, "x64-60gb"),
        }
    }
}

impl FromStr for ScwInstancesType {
    type Err = CommandError;

    fn from_str(s: &str) -> Result<ScwInstancesType, CommandError> {
        match s {
            "dev1-l" => Ok(ScwInstancesType::DEV1_L),
            "dev1-m" => Ok(ScwInstancesType::DEV1_M),
            "dev1-s" => Ok(ScwInstancesType::DEV1_S),
            "dev1-xl" => Ok(ScwInstancesType::DEV1_XL),
            "ent1-l" => Ok(ScwInstancesType::ENT1_L),
            "ent1-m" => Ok(ScwInstancesType::ENT1_M),
            "ent1-s" => Ok(ScwInstancesType::ENT1_S),
            "ent1-xs" => Ok(ScwInstancesType::ENT1_XS),
            "ent1-xxs" => Ok(ScwInstancesType::ENT1_XXS),
            "gp1-l" => Ok(ScwInstancesType::GP1_L),
            "gp1-m" => Ok(ScwInstancesType::GP1_M),
            "gp1-s" => Ok(ScwInstancesType::GP1_S),
            "gp1-viz" => Ok(ScwInstancesType::GP1_VIZ),
            "gp1-xs" => Ok(ScwInstancesType::GP1_XS),
            "gpu-3070-s" => Ok(ScwInstancesType::GPU_3070_S),
            "play2-micro" => Ok(ScwInstancesType::PLAY2_MICRO),
            "play2-nano" => Ok(ScwInstancesType::PLAY2_NANO),
            "play2-pico" => Ok(ScwInstancesType::PLAY2_PICO),
            "pro2-l" => Ok(ScwInstancesType::PRO2_L),
            "pro2-m" => Ok(ScwInstancesType::PRO2_M),
            "pro2-s" => Ok(ScwInstancesType::PRO2_S),
            "pro2-xs" => Ok(ScwInstancesType::PRO2_XS),
            "pro2-xxs" => Ok(ScwInstancesType::PRO2_XXS),
            "render-s" => Ok(ScwInstancesType::RENDER_S),
            "start1-l" => Ok(ScwInstancesType::START1_L),
            "start1-m" => Ok(ScwInstancesType::START1_M),
            "start1-s" => Ok(ScwInstancesType::START1_S),
            "vc1l" => Ok(ScwInstancesType::VC1L),
            "vc1m" => Ok(ScwInstancesType::VC1M),
            "vc1s" => Ok(ScwInstancesType::VC1S),
            "x64-120gb" => Ok(ScwInstancesType::X64_120GB),
            "x64-15gb" => Ok(ScwInstancesType::X64_15GB),
            "x64-30gb" => Ok(ScwInstancesType::X64_30GB),
            "x64-60gb" => Ok(ScwInstancesType::X64_60GB),
            _ => Err(CommandError::new_from_safe_message(format!(
                "`{s}` instance type is not supported"
            ))),
        }
    }
}
#[derive(Clone)]
pub struct ScwNodeGroup {
    pub name: String,
    pub id: Option<String>,
    pub min_nodes: i32,
    pub max_nodes: i32,
    pub instance_type: String,
    pub disk_size_in_gib: i32,
    pub status: scaleway_api_rs::models::scaleway_k8s_v1_pool::Status,
}

impl ScwNodeGroup {
    pub fn new(
        id: Option<String>,
        group_name: String,
        min_nodes: i32,
        max_nodes: i32,
        instance_type: String,
        disk_size_in_gib: i32,
        status: scaleway_api_rs::models::scaleway_k8s_v1_pool::Status,
    ) -> Result<Self, CommandError> {
        if min_nodes > max_nodes {
            let msg = format!(
                "The number of minimum nodes ({}) for group name {} is higher than maximum nodes ({})",
                &group_name, &min_nodes, &max_nodes
            );
            return Err(CommandError::new_from_safe_message(msg));
        }

        Ok(ScwNodeGroup {
            name: group_name,
            id,
            min_nodes,
            max_nodes,
            instance_type,
            disk_size_in_gib,
            status,
        })
    }
}

#[rustfmt::skip]
#[cfg(test)]
mod tests {
    use crate::cloud_provider::scaleway::kubernetes::node::ScwInstancesType;
    use crate::cloud_provider::kubernetes::InstanceType;
    use crate::cloud_provider::models::{CpuArchitecture, NodeGroups};
    use std::str::FromStr;
    use strum::IntoEnumIterator;

    #[test]
    fn test_scaleway_to_cloud_provider_format() {
        for instance_type in ScwInstancesType::iter() {
            // verify:
            // check if instance to AWS format is the proper one for all instance types
            let result_to_string = instance_type.to_cloud_provider_format();
            assert_eq!(
                match instance_type {
                    ScwInstancesType::DEV1_L => "dev1-l",
                    ScwInstancesType::DEV1_M => "dev1-m",
                    ScwInstancesType::DEV1_S => "dev1-s",
                    ScwInstancesType::DEV1_XL => "dev1-xl",
                    ScwInstancesType::ENT1_L => "ent1-l",
                    ScwInstancesType::ENT1_M => "ent1-m",
                    ScwInstancesType::ENT1_S => "ent1-s",
                    ScwInstancesType::ENT1_XS => "ent1-xs",
                    ScwInstancesType::ENT1_XXS => "ent1-xxs",
                    ScwInstancesType::GP1_L => "gp1-l",
                    ScwInstancesType::GP1_M => "gp1-m",
                    ScwInstancesType::GP1_S => "gp1-s",
                    ScwInstancesType::GP1_VIZ => "gp1-viz",
                    ScwInstancesType::GP1_XS => "gp1-xs",
                    ScwInstancesType::GPU_3070_S => "gpu-3070-s",
                    ScwInstancesType::PLAY2_MICRO => "play2-micro",
                    ScwInstancesType::PLAY2_NANO => "play2-nano",
                    ScwInstancesType::PLAY2_PICO => "play2-pico",
                    ScwInstancesType::PRO2_L => "pro2-l",
                    ScwInstancesType::PRO2_M => "pro2-m",
                    ScwInstancesType::PRO2_S => "pro2-s",
                    ScwInstancesType::PRO2_XS => "pro2-xs",
                    ScwInstancesType::PRO2_XXS => "pro2-xxs",
                    ScwInstancesType::RENDER_S => "render-s",
                    ScwInstancesType::START1_L => "start1-l",
                    ScwInstancesType::START1_M => "start1-m",
                    ScwInstancesType::START1_S => "start1-s",
                    ScwInstancesType::VC1L => "vc1l",
                    ScwInstancesType::VC1M => "vc1m",
                    ScwInstancesType::VC1S => "vc1s",
                    ScwInstancesType::X64_120GB => "x64-120gb",
                    ScwInstancesType::X64_15GB => "x64-15gb",
                    ScwInstancesType::X64_30GB => "x64-30gb",
                    ScwInstancesType::X64_60GB => "x64-60gb",
                }
                .to_string(),
                result_to_string
            );

            // then check the other way around
            match ScwInstancesType::from_str(&result_to_string) {
                Ok(result_instance_type) => assert_eq!(instance_type, result_instance_type),
                Err(_) => panic!(),
            }
        }
    }

    #[test]
    fn test_groups_nodes() {
        assert!(NodeGroups::new("".to_string(), 2, 1, "dev1-l".to_string(), 20, CpuArchitecture::AMD64).is_err());
        assert!(NodeGroups::new("".to_string(), 2, 2, "dev1-l".to_string(), 20, CpuArchitecture::AMD64).is_ok());
        assert!(NodeGroups::new("".to_string(), 2, 3, "dev1-l".to_string(), 20, CpuArchitecture::AMD64).is_ok());

        assert_eq!(
            NodeGroups::new("".to_string(), 2, 2, "dev1-l".to_string(), 20, CpuArchitecture::AMD64).unwrap(),
            NodeGroups {
                name: "".to_string(),
                id: None,
                min_nodes: 2,
                max_nodes: 2,
                instance_type: "dev1-l".to_string(),
                disk_size_in_gib: 20,
                desired_nodes: None,
                instance_architecture: CpuArchitecture::AMD64,
            }
        );
    }
}

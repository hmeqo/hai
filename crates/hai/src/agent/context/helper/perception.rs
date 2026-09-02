use std::collections::{HashMap, HashSet};

use uuid::Uuid;

use crate::{
    agent::context::types::{Attachment, AttachmentPerceptionMap, ParsedContent, PerceptionResult},
    domain::{model::Perception, service::DbServices, vo::resource_id_from_file_id},
    error::Result,
};

struct PerceptionLoader<'a> {
    services: &'a DbServices,
    perceptions: Vec<Perception>,
    seen: HashSet<Uuid>,
    by_attachment_id: HashMap<Uuid, Vec<Perception>>,
    same_resource_as: HashMap<Uuid, Uuid>,
}

impl<'a> PerceptionLoader<'a> {
    fn new(services: &'a DbServices) -> Self {
        Self {
            services,
            perceptions: Vec::new(),
            seen: HashSet::new(),
            by_attachment_id: HashMap::new(),
            same_resource_as: HashMap::new(),
        }
    }

    async fn load_file_attachments(&mut self, parts: &[&Attachment]) -> Result<()> {
        let file_ids: Vec<String> = parts.iter().map(|a| a.file_id.clone()).collect();
        let mut file_id_perceptions: HashMap<String, Vec<Perception>> = HashMap::new();
        for (fid, p) in self
            .services
            .perception
            .find_by_platform_file_ids(&file_ids)
            .await?
        {
            file_id_perceptions.entry(fid).or_default().push(p);
        }

        let mut first_file_attachment: HashMap<Uuid, Uuid> = HashMap::new();
        for att in parts {
            let file_uid = resource_id_from_file_id(&att.file_id);
            let hit = file_id_perceptions.get(&att.file_id);

            if let Some(&first) = first_file_attachment.get(&file_uid) {
                self.same_resource_as.insert(att.id, first);
            } else {
                first_file_attachment.insert(file_uid, att.id);
                if let Some(ps) = hit {
                    self.by_attachment_id.insert(att.id, ps.clone());
                }
            }

            for p in hit.into_iter().flatten() {
                if self.seen.insert(p.id) {
                    self.perceptions.push(p.clone());
                }
            }
        }
        Ok(())
    }

    async fn load_urls(&mut self, parsed: &[ParsedContent]) -> Result<()> {
        let urls: Vec<String> = parsed
            .iter()
            .flat_map(|p| p.text_fragments.iter())
            .flat_map(|text| super::search::extract_urls(text))
            .collect();

        if !urls.is_empty() {
            let url_perceptions = self.services.perception.find_by_urls(&urls).await?;
            for p in url_perceptions {
                if self.seen.insert(p.id) {
                    self.perceptions.push(p);
                }
            }
        }
        Ok(())
    }

    fn build_perception_result(self) -> PerceptionResult {
        PerceptionResult {
            items: self.perceptions,
            map: AttachmentPerceptionMap {
                by_attachment_id: self.by_attachment_id,
                same_resource_as: self.same_resource_as,
            },
        }
    }
}

pub async fn load_perceptions(
    services: &DbServices,
    parsed: &[ParsedContent],
) -> Result<PerceptionResult> {
    let mut loader = PerceptionLoader::new(services);

    let attachment_parts: Vec<&Attachment> =
        parsed.iter().flat_map(|p| p.attachments.iter()).collect();
    if !attachment_parts.is_empty() {
        loader.load_file_attachments(&attachment_parts).await?;
    }
    loader.load_urls(parsed).await?;

    Ok(loader.build_perception_result())
}

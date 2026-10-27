use drip_core::protocol::{Selection, SelectionSet, SelectionUpdate, SelectionUpdateResponse};
use reqwest::Method;

use crate::{
    engine::{EngineError, api},
    source::{random_id, same_path},
    transport::HttpTransport,
};

#[derive(Clone)]
pub(super) struct ActiveSelection {
    pub selection: Selection,
    pub generation: u64,
}

pub(super) async fn activate(
    transport: &HttpTransport,
    credential: &str,
    source: &drip_core::SourcePath,
) -> Result<ActiveSelection, EngineError> {
    let current = get(transport, credential).await?;
    let existing = current
        .selections
        .iter()
        .find(|selection| same_path(&selection.source, source));
    let selected = match existing {
        Some(selection) => selection.clone(),
        None => Selection {
            id: new_id(&current)?,
            source: source.clone(),
            active: true,
        },
    };
    let mut selections = current.selections;
    for selection in &mut selections {
        selection.active = selection.id == selected.id;
    }
    if !selections
        .iter()
        .any(|selection| selection.id == selected.id)
    {
        selections.push(selected.clone());
    }
    let next_generation = current
        .generation
        .checked_add(1)
        .ok_or(EngineError::InvalidResponse(
            "selection generation overflowed",
        ))?;
    let update = SelectionUpdate {
        expected_generation: current.generation,
        selections,
    };
    let result: SelectionUpdateResponse = api::json(
        transport,
        Method::PUT,
        "v1/selections",
        Some(credential),
        &[],
        Some(&update),
    )
    .await?;
    if result.generation != next_generation {
        return Err(EngineError::InvalidResponse(
            "selection update returned an unexpected generation",
        ));
    }
    Ok(ActiveSelection {
        selection: Selection {
            active: true,
            ..selected
        },
        generation: result.generation,
    })
}

pub(super) async fn require_current(
    transport: &HttpTransport,
    credential: &str,
    expected: &ActiveSelection,
) -> Result<(), EngineError> {
    let current = get(transport, credential).await?;
    if current.generation != expected.generation
        || !current.selections.iter().any(|selection| {
            selection.id == expected.selection.id
                && selection.active
                && selection.source == expected.selection.source
        })
    {
        return Err(EngineError::SelectionChanged);
    }
    Ok(())
}

pub(super) async fn fresh_barrier(
    transport: &HttpTransport,
    credential: &str,
    expected: &ActiveSelection,
) -> Result<ActiveSelection, EngineError> {
    let current = get(transport, credential).await?;
    if !current.selections.iter().any(|selection| {
        selection.id == expected.selection.id
            && selection.active
            && selection.source == expected.selection.source
    }) {
        return Err(EngineError::SelectionChanged);
    }
    let next_generation = current
        .generation
        .checked_add(1)
        .ok_or(EngineError::InvalidResponse(
            "selection generation overflowed",
        ))?;
    let update = SelectionUpdate {
        expected_generation: current.generation,
        selections: current.selections,
    };
    let result: SelectionUpdateResponse = api::json(
        transport,
        Method::PUT,
        "v1/selections",
        Some(credential),
        &[],
        Some(&update),
    )
    .await?;
    if result.generation != next_generation {
        return Err(EngineError::InvalidResponse(
            "selection barrier returned an unexpected generation",
        ));
    }
    Ok(ActiveSelection {
        selection: expected.selection.clone(),
        generation: result.generation,
    })
}

pub(super) async fn get(
    transport: &HttpTransport,
    credential: &str,
) -> Result<SelectionSet, EngineError> {
    api::json::<SelectionSet, ()>(
        transport,
        Method::GET,
        "v1/selections",
        Some(credential),
        &[],
        None,
    )
    .await
}

fn new_id(current: &SelectionSet) -> Result<String, EngineError> {
    for _ in 0..8 {
        let id = random_id()?;
        if current
            .selections
            .iter()
            .all(|selection| selection.id != id)
        {
            return Ok(id);
        }
    }
    Err(EngineError::InvalidResponse(
        "could not create a unique selection ID",
    ))
}

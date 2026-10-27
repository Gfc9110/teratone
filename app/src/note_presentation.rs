//! Remove only visually identical overlapping note rectangles from the view.
//! The full MIDI scene remains unchanged for selection and hit testing.
use crate::editor::NoteRect;

/// Preserve input order within each layer, with selected notes painted on top.
/// The ID is deliberately excluded: coincident notes remain separate in the
/// controller, even when their opaque visual representation is drawn once.
pub fn unique_notes(notes:&[NoteRect])->Vec<&NoteRect> {
    let mut result=Vec::with_capacity(notes.len());
    for selected in [false,true] {
        let mut previous=None;
        for note in notes.iter().filter(|note|note.selected==selected) {
            let key=(note.x.to_bits(),note.y.to_bits(),note.width.to_bits(),note.height.to_bits(),note.selected,note.label.as_str());
            // Only consecutive duplicates are safe to collapse: another note
            // between them may paint text or overlapping pixels that the later
            // rectangle is meant to cover.
            if previous!=Some(key) {result.push(note);}
            previous=Some(key);
        }
    }
    result
}
#[cfg(test)] mod tests {
    use super::*;
    fn note(id:u64,x:f32,selected:bool,label:&str)->NoteRect {
        NoteRect{id,x,y:20.0,width:4.0,height:14.0,selected,label:label.into()}
    }
    #[test] fn identical_rectangles_are_only_collapsed_in_presentation() {
        let notes=vec![note(1,0.0,false,"C4"),note(2,0.0,false,"C4"),note(3,1.0,false,"C4")];
        let result=unique_notes(&notes);
        assert_eq!(result.iter().map(|n|n.id).collect::<Vec<_>>(),vec![1,3]);
        assert_eq!(notes.len(),3);assert_eq!(notes[1].id,2);
    }
    #[test] fn distinguish_style_label_and_layer_and_keep_selection_on_top() {
        let mut different=note(4,0.0,false,"C4");different.height=12.0;
        let notes=vec![note(1,0.0,true,"C4"),note(2,0.0,false,"C4"),note(3,0.0,false,"D4"),different,note(5,0.0,true,"C4")];
        let result=unique_notes(&notes);
        assert_eq!(result.iter().map(|n|n.id).collect::<Vec<_>>(),vec![2,3,4,1]);
    }
    #[test] fn nonconsecutive_duplicate_is_retained_to_preserve_overlap_order() {
        let mut wide=note(2,0.0,false,"D4");wide.width=32.0;
        let notes=vec![note(1,1.0,false,"C4"),wide,note(3,1.0,false,"C4")];
        assert_eq!(unique_notes(&notes).iter().map(|n|n.id).collect::<Vec<_>>(),vec![1,2,3]);
    }
}

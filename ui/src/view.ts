// The 3D view's helpers the editor uses outside the review: its own entry
// ('@draftwright/specify-ui/view'), as their types reach three-cad-viewer's, which a host checking
// library types strictly may not accept with its own three. The root entry does not reach them.
// ``createCadView`` needs three-cad-viewer's stylesheet, which the root entry loads and this one does not (so the
// picker, look and the rest bundle without a CSS loader): a host that draws it from here loads 'three-cad-viewer/css'.
export { createCadView, pathOf, type CadNode, type CadView } from './specify/cad-view';
export { createPicker, type Picker } from './specify/picker';
export { createLook, type LookAt } from './specify/look';
export { createDiffDrawn, PMI_COLOUR, type DiffDrawn } from './specify/diff-drawn';

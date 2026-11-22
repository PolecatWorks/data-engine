import { createCanvasWrapper } from '../mfe-canvas.types';
import {
  SemanticTopologyComponent,
  SemanticTopologyState,
} from './semantic-topology.component';

const wrapper = createCanvasWrapper<SemanticTopologyState, SemanticTopologyComponent>(
  SemanticTopologyComponent,
);

export const mount = wrapper.mount;
export const unmount = wrapper.unmount;

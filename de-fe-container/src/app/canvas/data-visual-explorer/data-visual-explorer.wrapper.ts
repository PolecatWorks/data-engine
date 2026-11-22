import { createCanvasWrapper } from '../mfe-canvas.types';
import {
  DataVisualExplorerComponent,
  DataVisualExplorerState,
} from './data-visual-explorer.component';

const wrapper = createCanvasWrapper<DataVisualExplorerState, DataVisualExplorerComponent>(
  DataVisualExplorerComponent,
);

export const mount = wrapper.mount;
export const unmount = wrapper.unmount;

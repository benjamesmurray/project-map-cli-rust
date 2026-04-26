export interface ButtonProps {
    label: string;
}

export class Button {
    render(props: ButtonProps) {
        return `Rendering ${props.label}`;
    }
}

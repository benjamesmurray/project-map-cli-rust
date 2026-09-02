CREATE TABLE IF NOT EXISTS public.levels_history (
    id UUID PRIMARY KEY,
    price NUMERIC NOT NULL,
    volume NUMERIC
);

ALTER TABLE public.levels_history ADD COLUMN created_at TIMESTAMP;

CREATE VIEW public.active_levels AS
SELECT * FROM public.levels_history;

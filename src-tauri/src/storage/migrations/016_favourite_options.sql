-- How a favourite launches besides where (same server or spread, FPS cap, groups, hotkey), as
-- JSON (`model::FavouriteOptions`). NULL: the defaults.
ALTER TABLE favourites ADD COLUMN options TEXT;

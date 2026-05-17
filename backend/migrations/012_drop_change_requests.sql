-- The per-entry change-request workflow has been removed.
-- A submitted week is now treated atomically: individual entries inside it
-- cannot be edited, and any correction must go through the week-level
-- reopen workflow. The change_requests table and its indexes are no longer
-- referenced by application code and are dropped here.
DROP TABLE IF EXISTS change_requests;
